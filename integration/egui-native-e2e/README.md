# Native two-window end-to-end gate

This excluded workspace launches the fork-backed runtime with real OS windows.
Its event-loop test driver presses the root tab, moves and releases outside all
known windows, waits for the dynamically created child to present an
interaction-authoritative scene, then drags the child tab back into the root.
It passes only after the child surface has been retired, the root has recovered
the exact item multiset, and the root scene is interactive again.

This proves the dynamic tear-off, first-live admission, cross-window redock,
and source-vacancy vertical slice through the native event loop. It does not
simulate hardware input, transfer windows between mixed-DPI monitors, exercise
close/focus failure matrices, or prove multi-item ordering and grab-offset
preservation.

Run it from the repository root against the manifest-pinned fork revision:

```text
python3 scripts/run_native_e2e.py
```
