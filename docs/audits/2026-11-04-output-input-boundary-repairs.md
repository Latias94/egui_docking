# Output and input boundary repairs

Date: 2026-11-04. Status: completed and locally committed.
Baseline: `54bd389`. Implementation: `48b00d8`, `b50d0e6`. This follow-up
preserves the engine-first ordinary release contract and renderer-neutral core.
No remote publication is included.

## Ownership decisions

A reported newer paint can already have replaced the visible pixels before its
ordered report prefix reaches core publication. The runtime therefore withholds
older pointer authority immediately. Pending native staging does not replace
pixels, and a late report from an older endpoint stream cannot revoke its
successor. The existing output queue owns this barrier; no second scene registry
or replay watermark is introduced.

Candidate actions use the version they actually observe. Deferred splitter
actions retain their exact measurements and validate geometry separately from
item or root identity. A discarded pass can supply the first geometry for current
requirements; it cannot override a different accepted measurement or revive an
explicit unavailable observation. Hover continuation validates the current
surface/contained owner before scheduling selection.

A suspended document restore retains the complete original document binding.
A changed sidecar is a conflict on resume even if the workspace version stayed
the same. Retrying requires preparation against the current binding.

Egui input snapshots retain the first pass's post-input-hook events together
with framework widgets, interactive rectangles, transforms, modal and layer
clearance. Later speculative passes cannot silently substitute their receiver
facts. A synchronous run guard lets declared Area roots with the matching layer
identity enroll in that same pass. Ordinary foreign roots remain ineligible, including a
fresh second Dockspace instance. An input hook poisons raw recursive Context
runs before egui can panic on borrowed-root access; catching that panic cannot
recover authority in this run or its later retry passes. Event-time and current
coordinate transforms must both remain supported before interpreting a release.

Output proofs retain each layer's exact shape span. Complete-run final layer
order determines cross-participant ordering; callback order only determines
append order within a shared layer. Shape descriptors do not prove callback
internals or GPU presentation. Invisible Area sizing passes cannot grant
presented input authority. Failed owned runs preserve texture deltas for the next
renderer output, including consecutive set/free operations.

## Geometry and normalization

Hidden but draggable tab strips still require tab intrinsic measurements.
Overflowing edge previews use the same root/surface clipping as the final pane.
Splitter ties use geometric distance and stable identity. Touch partitioning
preserves visible bases, protects tab/title chrome from expanded halos, and
resolves near-coincident junctions without unowned holes.

The touch compiler uses a conservative sweep to omit disjoint pairs while
retaining the original exact partition order. A bounded neighbor budget falls
back to exhaustive scanning for dense overlaps. Differential tests compare every
partition fragment against exhaustive scanning; public tests cover visible
bases, protected chrome, clipping and front-window occlusion.

The stronger 5,000-step command model found a separate normalization defect:
canonicalizing an unchanged valid split repeatedly rounded its weights, turning
a no-op self-dock into a false layout change. Unchanged validated splits now
retain their exact weights. The public self-docking regression checks all four
edges, unchanged version and exact normalized shares.

## Validation

All serial gates passed on the measured source:

| Gate | Result |
| --- | --- |
| Workspace, all features/all targets | 2,056 passed |
| Public host, default / persistence | 289 / 332 passed |
| Official egui public consumer | 24 passed |
| Egui product consumer, default / persistence | 114 / 119 passed |
| Fork-backed native adapter, all features/all targets | 556 passed; one pre-existing ignored test |
| Correctness/suspicious Clippy, workspace and native | Passed |
| Strict documentation, core/egui/native | Passed |
| Workspace doctests | Nine core and one adapter passed |
| Renderer-neutral dependency boundary | Passed |
| Cargo fmt and diff checks | Passed |

The three pre-existing 10,000-depth public host stress cases were excluded from
the host counts and were not rerun in this follow-up. The actual X11/editor
qualification remains the prior dated baseline; this follow-up does not claim
a fresh physical GUI run on macOS or X11.

Independent read-only core review found no evidenced P1/P2 blocker in the final
changes. Adapter review found two additional scope defects, which were fixed
with public regressions; its follow-up found no remaining concrete P1/P2 in
that scope. Review and tests do not prove the absence of all possible defects.

The [current-source performance record](../performance/2026-11-04-boundary-repair-results.md)
retains the CSV, compiler/host metadata and exact source inventory. At 1,024 panes,
changed-update p50 is 48.337–61.181 ms and four retained outputs occupy
127.551 MiB of tracked heap, falling to 20.259 MiB after settlement. This improves
the recorded update time but remains above a real-time frame budget and retains
more memory than the older frozen implementation.
