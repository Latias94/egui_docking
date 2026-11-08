# Native recovery and compact pane menus

This follow-up starts from `ec9fe1d`. Actual editor checks cover layout recovery,
native drag cancellation and a smaller host window. Recovery and cancellation
preserve their existing contracts. Compact inventory testing exposed two defects
that are corrected together.

| Before | After |
| --- | --- |
| Inventory placed a pane's title and every action on one horizontal row. At 440 pixels, the rightmost content control extended past the host window. | Each named pane opens a standard egui submenu with vertically arranged actions. Movement commands appear directly in that submenu. |
| A foreground menu could block a core pane while providing no egui drag receiver. The native adapter reported `NoReceiver`, contradicting the pane's core drag geometry and stopping the runtime on pointer press. | A missing lane checks the same completed-pass foreground identity. An undeclared framework widget reports `Blocked`; ambiguous registration remains `Unknown`. |

## Inventory interaction

The inventory still identifies all eight application panes and their
selected/open/closed state. Its pane submenu retains selection, docking,
floating, group raising/recovery, pane/group close, reopening and content
unload/reload. This replaces the wide row and its separate Move submenu; it does
not introduce menu state or a command executor.

Arrangement lock leaves selection and content availability independent of layout
changes. Pending work disables further actions through the existing bounded
command/result slot. Closing still uses core-issued decisions and reopening
retains application data. The walkthrough describes the new pane menu.

The controls regression opens the real inventory with pointer input, hovers
Inspector and checks that the complete content action fits in a 440 × 620 egui
viewport. Activating the action unloads Inspector alone. The old layout fails
with an action right edge at `458.21875`, beyond the 440-pixel window. Another
test checks all four arrangement-lock/pending combinations.

## Native foreground blocking

The actual-window check initially failed before the content toggle ran. Movement
into the submenu succeeded; pointer press alone stopped the runtime. Temporary
error inspection identified:

```text
InputPrefixReductionFailed
PointerReceiverGeometry::ReceiverWinnerMismatch
lane: Drag, claimed: None, winner: Some(PaneBody(...))
```

An egui click-only button can occlude underlying content without supplying a
drag receiver. Absence of that lane therefore cannot prove absence of core
geometry. `NativeReceiverStore::resolve` now reuses its existing exact
viewport/pass/widget/layer lookup for the completed pass's `contains_pointer`
identity when the requested lane is absent. A foreign identity blocks docking.
An ambiguous registered identity remains unknown. No foreground hit, or a
uniquely registered hit with no requested lane, retains `NoReceiver`.

Existing nonempty-lane resolution, scroll routing and hover clearance keep their
current paths. The correction neither fabricates a menu drag capability nor
relaxes core geometry checks. Core sources have no final change. Temporary
diagnostic logging and process-exit probes were removed.

The native regression reuses the actual surface-paint fixture with click-only
and hover-only foreground overlays. It explicitly checks that the drag lane is
absent, that Click and Drag queries both run, and that blocked native press and
release commit. The subsequent ordinary tab-drag/hover checks still execute.
The old resolver fails the click-only case by returning `NoReceiver` instead of
`Blocked`. All 16 receiver tests pass after correction.

The final ordinary 440-pixel editor received real mouse input through inventory,
Inspector, Unload content, Reload content, Close, the explicit confirmation and
Reopen. It remained responsive, reported each content toggle, required close
approval and reopened Inspector in the main dockspace with its notes available.
The [finite input journey](2026-11-08-native-recovery/compact_inventory_journey.py)
asserts the preserved host identity, 440-pixel width and live process. Widget
state and action feedback were checked in the original screenshots, separately
from those OS assertions:

| Actual-window observation | Evidence |
| --- | --- |
| Original wide inventory clips the content action | [Before](2026-11-08-native-recovery/compact-inventory-before.png) |
| Compact Inspector menu shows every action | [Menu](2026-11-08-native-recovery/inspector-actions.png) |
| Pointer press stopped the runtime before the native correction | [Failure](2026-11-08-native-recovery/native-menu-stopped-before.png), [diagnostic](2026-11-08-native-recovery/native-menu-diagnostic.txt) |
| Corrected runtime applies unload and reload | [Unloaded](2026-11-08-native-recovery/inspector-unloaded.png), [reloaded](2026-11-08-native-recovery/inspector-reloaded.png) |
| Closing requires an explicit named decision | [Confirmation](2026-11-08-native-recovery/inspector-close-request.png) |
| Approved close offers Reopen, then Inspector returns with its notes | [Closed menu](2026-11-08-native-recovery/inspector-reopen-menu.png), [reopened](2026-11-08-native-recovery/inspector-reopened.png) |

## Recovery observations

The existing root-rebinding journey exercised 12 actual focused edits across six
child-window lifecycles. It checked two simultaneously live children, JSON
restoration, destruction of old children, fresh child editing, root identity
retention and quiescence. Some layout commands in this existing smoke are
programmatic; its text entry and native window closure use OS input.
It passed at the baseline and was rerun successfully with the final binary;
the [final receipt](2026-11-08-native-recovery/root-rebinding-final.json) and
[raw log](2026-11-08-native-recovery/root-rebinding-final.log.gz) identify that
execution.

Separate ordinary mouse/key journeys checked individual-tab and complete-root
group drags from a native child. These two cancellation journeys were run only
at baseline `ec9fe1d`, as bound by the
[baseline window state](2026-11-08-native-recovery/baseline-window-state.json);
they were not repeated after the native receiver correction. Both entered a
visible drag before Escape and pointer release. The source panes and exact child
survived, guides disappeared,
and toolbar save/default/restore created a fresh child with retained text. After
complete-root cancellation, a real content click and text entry confirmed that
native input hit testing recovered. Escape's existing egui focus-clearing
behavior is preserved; cancellation does not request replacement focus.

## Verification and evidence

Final execution receipts, source/binary identities and selected original
screenshots are recorded in the adjacent
[`2026-11-08-native-recovery`](2026-11-08-native-recovery/) directory.
Automated native/window assertions and visual inspection are recorded
separately. The independent Standards and Spec reviews found no remaining
static findings, including after the foreground-blocking correction.

| Final gate | Result |
| --- | --- |
| Fork-backed native workspace, all features and targets | 615 passed; 1 existing skip |
| Native package, default features and all targets | 564 passed; 1 existing skip |
| Native workspace correctness/suspicious Clippy | Passed |
| Native workspace cargo fmt and git diff checks | Passed |
| Linux all-feature example build and all-feature/all-target workspace check | Passed |
| Ordinary compact-menu actual-window journey | Passed OS assertions and screenshot inspection |
| Final native recovery/root-rebinding journey | 12 real edits and 6 child lifecycles; root retained, fresh presentation and quiescence verified |

This work uses the existing one-CPU, 4 GiB Linux/Xvfb/Openbox software-GL setup.
It does not measure docking performance or qualify additional platforms.
Cargo checks run serially with one build job and nextest worker, using the
existing target directories. Shared core and portable sources are unchanged.
The unrelated existing `docs/reviews/` directory is excluded from this work.
