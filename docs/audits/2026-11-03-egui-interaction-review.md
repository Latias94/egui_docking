# Native egui interaction and visual continuity review

Review baseline: `63112e044f40402de0331f1a653a875bcae93840`.

This review follows a manual report that moving windows and selecting tabs flash
the dockspace, and that dragged tabs cannot be docked back reliably. It audits
the complete egui/native path, rather than treating a correct final graph or a
successful lifecycle smoke as evidence of usable intermediate frames.

## Findings

### P1: measurement was presented as an empty workspace

Changing selection or native coordinate authority invalidates the current scene.
`native_support::paint_surface` returns a background-only paint when no current
plan exists. The native driver used to draw this output, commit measurement,
and leave the renderer in its default replacement mode. The following callback
painted the replacement scene. No previous-frame retention covered the gap.

The `presentation_continuity` native example reproduces this using real
eframe/Glow callbacks and the public selection API. The first selection fails
with `selection 1 produced a blank frame`. The recording counts actual pane
callbacks and distinguishes a staged retained pass from a replacement pass;
it does not fabricate renderer presentation or native facts.

The repair separates measurement from painting within one application UI call:
commit a measurement-only core frame, then paint a new core frame on the same
native output token and the same unconsumed Ui. The measurement phase calls no
application pane UI. It neither produces a candidate execution receipt nor
promotes stale input authority. If native work cannot advance, the existing
retention path remains responsible for the actual previous framebuffer.

### P2: a ready native scene could indefinitely retain outdated host geometry

The native driver previously measured only when a current scene was absent.
Changing the space occupied by the host toolbar, a dynamic title or a pane
minimum did not necessarily invalidate that scene. The renderer could therefore
draw current text into obsolete tab and content geometry.

Normal native semantic passes now use the same measurement-before-paint ordering
as the official adapter. Active manipulation and pending preview presentation
keep their frozen geometry until their existing continuation settles. Unchanged
measurements reuse the existing scene machinery; no extra layout graph or
authority registry is introduced.

### P1: declared hover-only pane content was rejected as a docking surface

The native resolver examines the exact presented pass's topmost hover widget.
The pane renderer accepted application declarations only for interactive
widgets. An explicitly declared `Sense::hover()` label or canvas was therefore
missing from the receiver set and returned `Blocked`. A drag preview was cleared
and release could cancel with `OpaquePointerBlocker` over ordinary pane content.

The repair must admit exact, enabled, same-layer, clipped pane-owned hover
declarations without treating unknown widgets, popups or foreign overlays as
transparent. The native basic example must also declare its ordinary content,
including headings and log/profiler widgets, at the actual application callback.

Independent review caught a second boundary while qualifying this change: a
declared hover-only widget can sit in front while an undeclared widget still
owns the click or drag lane at the same point. A maintained native regression
fails with `ClearHoverLane` where `Blocked` is required. Native hover clearance
now checks every active hit lane against the exact presented receiver set.
The all-declared positive case remains accepted; unknown and foreign identities
cannot borrow clearance from a label or container.

### P2: contained tab redocking required leaving the entire original window

Dragging all tabs of a contained window used `AwaitContainedExit`, based on the
frozen original contained rectangle. A contained window created from a full
workspace pane can occupy the complete host surface. With local pointer routing,
no ordinary in-surface target then satisfies that exit condition. Exact guide
and tab-gap targets remain a workaround, but ordinary magnetic redocking is
effectively inaccessible.

The source gate must express the user's gesture intent. A tab/content drag may
resolve destinations underneath its moving source; a title-bar window move can
continue to require an explicit docking target. Source-tree exclusion already
belongs to the canonical drop resolver.

## Comparison with ImGui

The relevant ImGui reference is the checked-out docking branch under
`repo-ref/imgui` at `d88f238d1d363ef387382b2e29ad12cf96f826ac`, particularly the moving-window updates, docking target
selection, and tab layout in `imgui.cpp` and `imgui_widgets.cpp`. Its moving
source and docking target remain drawable while input changes. The reference
supports separating continuity of visible content from eligibility to authorize
a new drop. It does not justify weakening exact-output settlement or admitting
foreign overlays.

The native driver must not replay arbitrary application pane callbacks through
a stale plan merely to hide a blank frame. Measuring and drawing current
content in the same application pass preserves the existing ownership model.

## Verification

The original native continuity reproducer fails on the first selection before
the measurement repair. The same real macOS eframe/Glow example passes ten tab
selections, ten real OS-window position changes and four host layout changes,
both with default egui passes and with `--single-pass`. It checks every
application pass for actual pane callbacks or explicit retention/discard, and
checks the pane's bounds against the current host Ui. Staged retention is an
intent until the renderer accepts it; this probe does not claim pixel equality
or prove physical macOS mouse input.

Both full-surface contained-redocking product journeys fail before removing the
source-exit gate, then pass afterward. The native declared-hover regression
fails before the registration repair. Independent review's overlapping unknown
interactive-widget counterexample also fails before its repair. All 15 native
receiver tests then pass, including all-declared positive, undeclared negative,
foreground overlay, duplicate, wrong-artifact and old-emission cases.

Final qualification passes 2,005 workspace tests, 119 product consumer tests,
593 native-workspace tests (one platform-specific skip), 15 official-egui
consumer tests, ten doctests and both core/native Clippy correctness/suspicious
checks. Strict native rustdoc and format checks also pass. The actual X11 input
example completes the full-surface tear-off and ordinary-pane redock with exact
final ownership and no blank content passes. Real macOS default/bounded child
redock and all six output-pressure profiles pass after the scheduling repair.
The final results and maintained commands are recorded in `docs/VALIDATION.md`.

## Additional review boundary

The drag-decoration renderer currently reads egui's current hover position,
separately from journal-owned movement. Its behavior when these facts disagree
needs an explicit surface-coordinate and visibility test before changing it.
The static observation alone is not treated as proof of the reported redocking
failure.

macOS physical cross-OS-window dragging remains a separate unsupported lane:
the current backend does not advertise authoritative button/capture facts.
This limitation does not excuse same-window docking, floating-window movement
or programmatic child-window hosting failures.
