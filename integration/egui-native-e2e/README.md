# Native two-window end-to-end gate

This excluded workspace launches the fork-backed runtime with real OS windows.
Its event-loop test driver presses the root tab-group grip, moves and releases
outside all known windows, waits for the dynamically created child to present
an interaction-authoritative scene, then drags the complete child tab group
back into the root. It verifies tab order, selection, and MRU both in the live
child and after recovery. It passes only after the child surface has been
retired, the root has recovered the exact payload, and the root scene is
interactive again. It then routes a positioned `WindowEvent::MouseWheel`
through the production platform journal and egui derivative claim, and verifies
the resulting core-owned tab-strip offset.

This proves the dynamic tear-off, first-live admission, cross-window redock,
source-vacancy, multi-item stack order, and wheel-derivative vertical slices
through the native event loop. It does not simulate hardware input, transfer
windows between mixed-DPI monitors, exercise close/focus failure matrices, or
prove grab-offset preservation.

Run it from the repository root against the manifest-pinned fork revision:

```text
python3 scripts/run_native_e2e.py
```
