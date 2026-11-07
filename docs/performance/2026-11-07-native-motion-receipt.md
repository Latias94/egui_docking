# Observe real UI-consumed motion after hover has left

The physical drag-back barrier demanded a current latest pointer position.
The pinned input path legally permits Move followed by PointerGone while
primary remains down and the product drag continues. Current hover can expire
before a later inspection response. The latest failed run has 147 successful
Child trees and a final absent pointer label; it does not establish the exact
native Move/Gone/ACK order or a product delivery defect.

Keep the original hover/press/release state diagnostic. For 14 held-motion
injections, capture a UI-published baseline before the same physical Move,
then require a newer exact-viewport tree with a strictly newer consumed Move,
actual coordinate, matching window lifetime and held button state, with no
intervening release/cancel. One smoke-private record per active viewport is
populated only by the real processed InputState.raw.events. Gone retains the
Move; empty/repeated passes create no new event sequence. A passive AccessKit
label and typed UI record must agree on sequence, preventing an old tree from
borrowing a newer shared observation. Host callbacks and expected coordinates
do not create receipts or product authority.

The original 15 state barriers, physical assertions, three-second deadlines,
route/enable/restore ACKs, Center preview, release cleanup and quiescent
retirement remain. No product/core/native implementation, fork, dependency
or inspection protocol changes. Two independent source reviews find no
blocker and independently identify the redundant latest-token field, removed
in the lean treatment while retaining its actual lifetime anchor.

Local cargo fmt and all 32 physical-binary unit tests pass on macOS, including
three actual Context/plugin/AccessKit regressions for same-batch and later
Gone, empty/repeated passes, old/foreign/wrong-coordinate observations,
Move-before-press, release/repress and explicit cancellation. The first
execution had 29 passes and three texture-delta-disposal panics in the new
renderer-free tests. The fixture now explicitly clears FullOutput textures
through the pinned API; runtime and assertions are unchanged. Both attempts
and the small source adaptation are preserved.

Linux app/driver and real X11/XTest execution remain the fresh remote-CI
validation boundary. These local tests are not a native physical success or
proof of the historical failure's unique initiating sequence.

The [evidence archive](2026-11-07-native-motion-receipt/evidence.tar.xz)
binds the original and lean packages, both independent reviews, exact final
sources, first local failure and successful local qualification. The separate
[failed-run analysis](2026-11-07-small-root-challenge.md) preserves actual CI
trace evidence and its limits.

The [final test-adaptation review](2026-11-07-native-motion-receipt/test-adaptation-review.md)
confirms only explicit test texture disposal after formatting. Production
code and all assertions retain their reviewed bytes; final hashes match the
executed qualification receipt.
