# Native two-window end-to-end gate

This excluded workspace launches the fork-backed runtime with real OS windows.
Its event-loop test driver presses the root tab-group grip, moves and releases
outside all known windows, waits for the dynamically created child to present
an interaction-authoritative scene, then drags the complete child tab group
back into the root. It passes only after the child surface has been retired,
the root has recovered the complete payload on the requested edge, and the root
scene is interactive again.

This proves the dynamic tear-off, first-live admission, cross-window redock,
source-vacancy, and multi-item payload vertical slice through the native event
loop. Detailed topology, input-journal, scrolling, retention, and scheduling
contracts remain in focused Rust tests rather than this smoke. It does not
simulate hardware input, transfer windows between mixed-DPI monitors, exercise
close/focus failure matrices, or prove grab-offset preservation.

Run it from the repository root against the shared fork-workspace pin:

```text
python3 scripts/run_native_e2e.py
```
