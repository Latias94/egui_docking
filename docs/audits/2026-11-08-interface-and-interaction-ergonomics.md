# Interface and interaction ergonomics

This review starts from `2697b04` and the actual official-egui editor. It traces
caller setup, ordinary content declarations, composed application UI, close and
recovery, floating chrome and overflow navigation. Three parallel source-review
lanes covered interfaces, interaction and architecture; implementation and
verification were coordinated serially.

The changes remove caller bookkeeping and repair concrete interaction failures.
They do not replace docking algorithms, relax presentation authority, introduce
animations, or claim a new ImGui performance result.

## Implemented changes

| Before | After | Why |
| --- | --- | --- |
| Each pane stored and cleared widget IDs so a separate `docking_content_widgets` callback could immediately return them. | `PaneView::ui` returns a `PaneResponse` from that same callback. The old getter and production per-pane ID caches are removed. | Ownership is local to the rendering call; simple panes need one returned response. |
| The atomic owned driver could only paint the full docking surface. | `run_single_surface_with_ui` composes application panels before docking, using the remaining Ui. The original entry point delegates to this same driver. | Editors can add toolbars, sidebars and status panels without switching to a weaker publication path. |
| The official demo displayed close controls but discarded the close-request response. | Its explicit sample policy approves pending close decisions while retaining application documents. | Close controls now complete the operation they advertise. Applications still own Allow/Veto/Defer. |
| Closing both roots could remove the last surface and make the restoration toolbar unreachable. | An empty workspace paints the same toolbar and an all-closed message through the existing egui output owner. The actual Restore button recreates the layout without clearing edits. | Recovery remains available after the final group closes. |
| Floating titles used the resize-inset drag rectangle and vertical window margins for Heading text. The default Inspector title was visibly cut through its glyphs. | Text uses the complete title height, horizontal margins, a separate grip gutter and one truncated line. | Titles remain readable without changing resize, close, menu or drag hit geometry. |
| Backward overflow scrolled toward earlier tabs but painted a right-facing chevron; forward had the reverse mismatch. | Chevron direction matches the existing command. | The visual cue agrees with the resulting movement. |
| Clipped titles and several icon controls lacked mouse-readable descriptions. | Full title and purpose tooltips cover tabs, close, group drag, overflow and splitters. | Users can identify truncated content and distinguish tab overflow from window actions. |
| Native construction accepted a DockStyle that disagreed with the session's geometry or timing. | Construction validates the style and rejects the first represented-field mismatch before enrollment. | The renderer cannot silently present metrics different from the core hit and timing configuration. |
| Builder state wrapped one layout in a single-variant enum, and three construction paths repeated adapter initialization. | The builder stores the layout directly; one private `from_session` initializes adapter state. | Removes redundant implementation without adding another public construction abstraction. |

## Interface migration

The pane callback change is intentionally breaking. Return ordinary content
registrations directly:

```rust,ignore
fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
    let document = self.documents.get_mut(&item).expect("registered document");
    let editor = ui.text_edit_multiline(&mut document.text);
    PaneResponse::from(&editor)
}
```

Use `with_content` and `with_content_ids` for composite controls, or an empty
response to preserve undeclared-content behavior. The first ID is stored inline;
the ordinary single-control path does not allocate an ID vector. Additional
registrations retain their actual order and duplicates. Test-only observations
may still retain IDs when those observations are part of a behavioral assertion.

Declarations remain application ownership facts. The renderer checks current
registration, enabled state, exact layer and valid contained interaction bounds.
It does not infer arbitrary widget ancestry, authorize overlays, or guarantee
that every incorrectly declared same-layer widget can be recognized as foreign.
Focus hooks remain separate because native focus sampling also occurs before
the pane rendering callback.

The composition callback executes once per actual egui pass. It shares the
original candidate, ordered input, output-hook validation and presentation
receipt. Caller state and callback side effects cannot be rolled back by a
rejected output. The editor collects a restore intent across repeated passes
and applies it after successful output production and actual presentation.

Native users can initialize matching core settings through the now-public
`DockStyle::presentation_config()`. The native comparison covers the same 25
mapping entries, including both minimum-size components. It deliberately leaves
core-only drag thresholds, scroll-line extent and magnetic targeting independent,
and does not compare colors or ghost offset. Style geometry is promoted exactly
from `f32` to `f64`; a mismatch returns InvalidConfiguration with the field name.
Invalid style retains its original error source. No existing session is rewritten
to conceal disagreement.

The [migration guide](../ENGINE_FIRST_MIGRATION.md) and adapter/official-host
READMEs describe these final interfaces. Existing dated performance reports
continue to describe their own source checkpoints.

## Verification

Serial local verification passed:

| Gate | Result |
| --- | --- |
| Workspace, all features and targets | 2,252 tests passed; 3 pre-existing skips |
| Independent official-egui consumer and demo | 27 tests passed |
| Independent product consumer, all features and targets | 119 tests passed |
| Fork-backed native workspace, all features and targets | 599 tests passed; 1 pre-existing skip |
| Workspace doctests | 11 tests passed, including the new PaneResponse example |
| Workspace, official demo and native correctness/suspicious Clippy | Passed |
| Formatting | Cargo fmt applied across the affected workspaces |

The new checks exercise actual output rather than matching private helper
implementations: title glyphs fit with ordinary and enlarged vertical window
margins; emitted chevrons face the matching semantic direction; top/bottom
panels leave the actual core and pane bounds in their remaining rectangle;
composition retains multipass and late-hook rejection behavior; native style
validation preserves core-only settings and error sources.

The editor tests activate real AccessKit close controls and the window menu,
resolve application decisions, close both roots to zero surfaces, inspect the
painted empty state, then activate its actual Restore button. The ordinary
close/restore test independently retains unsaved document text and stable items.
The continuous six-phase journey uses the same composed editor helper.

These semantic tests own CPU-test presentation acknowledgements. They do not
establish GPU presentation or physical OS input. Actual-window evidence and
screenshots are recorded alongside the final run artifacts in
[`2026-11-08-ergonomics`](2026-11-08-ergonomics/).

The rebuilt production WGPU host completed the six-phase journey in 47.39
seconds with 384 retained rows and zero startup drops. Every scenario has its
complete sample indices 0–63 and nonempty actual meshes. This uses the same
one-CPU, 4 GiB Linux/Xvfb/llvmpipe environment with `LP_NUM_THREADS=1` as the
earlier host experiment. The toolbar changes the workload; the retained timings
are not a before/after docking-performance comparison or hardware-GPU result.

After that run, a separate isolated X11 window received ordinary pointer/key
input to insert `~ERGONOMICS~`, close Editor and activate Restore layout. Visual
inspection of the screenshots confirms the closed tab is removed, the restored
tab retains the inserted text, and the floating title is legible. A stationary
splitter hover also displays its purpose and keyboard-adjustment tooltip.
The capture script is a finite evidence artifact, not a new reusable GUI runner.
Its window-manager property warnings are preserved in stderr; the editor
remained live and the captured actions completed.

| Visual check | Evidence |
| --- | --- |
| Original clipped floating title | [Baseline](2026-11-08-ergonomics/baseline.png) |
| Composed toolbar and readable floating title | [After](2026-11-08-ergonomics/after.png) |
| Actual edit before closing | [Edited](2026-11-08-ergonomics/edited.png) |
| Editor removed and Preview selected | [Closed](2026-11-08-ergonomics/closed.png) |
| Restored arrangement with the inserted text retained | [Restored](2026-11-08-ergonomics/restored.png) |
| Stationary splitter feedback | [Tooltip](2026-11-08-ergonomics/splitter-tooltip.png) |

Commands and outcomes are retained in
[verification.json](2026-11-08-ergonomics/verification.json), with compressed raw
test/lint logs. The [actual-window receipt](2026-11-08-ergonomics/actual-journey.json)
binds the source used by the host run and its complete [CSV](2026-11-08-ergonomics/frames.csv).
Independent [interface/host review](2026-11-08-ergonomics/code-review.md),
[UI/construction review](2026-11-08-ergonomics/ui-review.md), and
[native configuration review](2026-11-08-ergonomics/native-config-review.md)
record their own scopes and the empty-workspace correction.
A final [report/evidence check](2026-11-08-ergonomics/evidence-review.md)
reconciles the retained test counts, source hashes, actual-window receipts and
screenshots with this report.

## Remaining design opportunity

The native application command facade still exposes a narrower inventory than
the portable facade: item docking/floating and contained raise/bring-into-view
can be made easier for application menus and command palettes. A follow-up
should use the existing action queue and its Busy/Stopped/result/stale checks,
with public consumer scenarios for those operations. This review does not
justify replacing the core evaluator, inventing another command system, or
expanding platform support claims.
