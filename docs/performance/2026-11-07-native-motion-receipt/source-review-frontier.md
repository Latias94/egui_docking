# Native motion receipt: independent source review

No substantive source blocker was found in the original stationary package. The change replaces an expiring hover-state observation with a receipt of a real UI-consumed Move, while preserving the separate state barriers and product settlement requirements. One redundant field/check can be removed as described below. This review does not establish that the failed CI run had the proposed Move/Gone ordering, that native delivery has been repaired, or that this package compiles or passes a physical smoke.

The reviewed original patch is `70656ac0269be13f06be277593ea6a1551703bd6066ac7aaa6786e87b2268eff`, against immutable parent `9ddb63f425ca8316bdf483d6297e923c35a16ac3`. The manifest hash is `a575052b45b7e2587f7e3f8275877b2bcd530a478fa8bfb21ccf3c6dc787e659`. I read the two failure analyses, the complete candidate, its immutable base and unchanged context, and the pinned fork APIs. I did not read `review-minimal`, run Cargo/compiler/tests/formatting/timing/network/runner, execute candidate scripts, or write any source. Only this report was written.

## Binding and scope

The three existing base files match the immutable parent, including their Git blob IDs, sizes and SHA-256 values. The fourth path does not exist at that parent. All four candidate hashes and Git blob IDs, all five unchanged context files, artifact hashes, READY bindings and both source-set digests were independently checked. Every patch hunk was reconstructed in memory at its exact original position and reproduced the complete candidate files.

| Source under `integration/egui-native-smoke/src/bin/physical_x11/` | Candidate SHA-256 | Delta |
| --- | --- | --- |
| `app.rs` | `2881b89e63c615542c36268ee97e887a669ed8579f2d4f4d8fbd05ef2ec52350` | +8/-0 |
| `assertions.rs` | `981dfae5b1f8e3f6bae4ddeb45ab84e5fb92c473e4ae8aff9395414da6505733` | +3/-0 |
| `driver/scenario.rs` | `6f27bdc9bbb3409f19fbbe8e8dc15ff1fec1a530c5a17d0bf6a700f5ee378b23` | +123/-58 |
| `motion_receipt.rs` | `6a2f57eb21a7d9b43cb38dabb198a43e5a9bfe7e9541fd121b6cd8117fb4e91b` | +427/-0 |

Totals match the announcement: +561/-58, including 225 test lines; runtime +336/-58. Base-set digest is `c33a7a3e906544cd77f12b38593fc8ae1e456bcb00511bb2ba30d4b24ea18a9e`; candidate-set digest is `5d26e7e472a53981a071dce76cd6f5cd4816bc3ed602fa9a735199e5f2dbec7c`. No product/core/native-runtime, fork, dependency, inspection-protocol or deadline implementation is changed. All pre-existing bytes in `app.rs` and `assertions.rs` remain in their original order.

## Receipt semantics and rejection boundaries

The only production publisher is the existing `PhysicalSemanticProbe::on_begin_pass`, after egui has processed the selected viewport's input. It reads `InputState.raw.events` in their actual order. The publisher receives neither an expected coordinate nor a driver baseline. Native callbacks do not manufacture receipt facts. The record's position is the actual `PointerMoved` payload, and its held flag is the tracked primary state at that event's position in the batch.

The important closed-path checks are:

| Boundary | Actual enforcement |
| --- | --- |
| Old Move or no new Move | `move_sequence > baseline.sequence`, rather than merely a newer tree or newer Gone/button sequence |
| Move before press | `moved_held` must be true even if the final pointer state becomes held later in that batch |
| Later primary release | Release advances the sequence, clears held state and records a new cancellation sequence |
| Release followed by repress | The cancellation sequence remains different from the baseline; repress does not rehabilitate the earlier gesture |
| Focus loss or Escape | The ordered event advances cancellation and clears the probe's primary state |
| Gone after a held Move | Gone advances its own fact/sequence without releasing the held Move; a fresh tree may report that occurrence after hover expires |
| Wrong coordinate | Existing checked semantic bounds and pointer tolerance helpers compare the actual recorded coordinate with the expected physical projection |
| Wrong viewport | The stored receipt and baseline must both identify the requested viewport; the unchanged inspection client rejects another returned viewport |
| Native context/window/create-attempt change | The publisher resets the record before processing that lifetime's input; the saved lifetime anchor no longer equals the baseline |
| Viewport removal | The map is pruned against actual raw viewport inventory; a later record begins again |
| Frame/pass counter restart | Reset clears the old record and anchors the next native output token, invalidating the old baseline |

The record is additionally intersected with egui's current `pointer.primary_down()`. A newer release/cancellation observed before the shared-fact read therefore cannot be combined with an earlier successful tree: either its sequence no longer matches the sample, its cancellation differs, or its held condition fails. After saturation, strict sequence advancement fails closed.

This is a source-viewport UI-input observation contract. It is not a replacement for the native gesture owner's cross-window cancellation, admission or terminal ledger. A cancellation which has not entered this UI input path is not independently certified by this record; the unchanged native/product route and settlement assertions still decide the actual operation.

## Lifetime, output freshness and authority

I checked the local checkout at exact egui pin `f94ee58b3914de7ad71dab36919b329ef36b4035`. `NativeOutputToken` is an opaque copyable value containing context, nonce, viewport, window and optional create attempt. Its public `same_context`, `viewport_id`, `window_id` and `create_attempt` methods support the comparisons used here. The publisher first filters the active token to the actual current UI viewport. It retains an initial token as the lifetime anchor only while context/window/create-attempt identity remains equal. Presence-to-absence or absence-to-presence of native scope resets the record.

The production injection helper rejects an absent native lifetime or unheld baseline before moving the physical pointer. The `None` lifetime accepted by the private matching method is used by the unmanaged Context tests; production cannot enter that branch through `inject_held_motion`. No native token is fabricated or decoded.

Each motion barrier still obtains an exact-viewport `GetViewportTree` and requires its step to exceed the original preceding checkpoint. The pinned inspection plugin only completes that request from the selected viewport's new output; the unchanged client checks the returned viewport and requires an AccessKit tree. The new sample must have the same input sequence as the latest typed UI record, and its bounds must encode that record's actual Move position. Thus a newer shared Move/release/Gone cannot be silently paired with an older sequence from the returned tree.

The sample carries input sequence and position, not a serialized token or a claim of exact output-token equality. Empty successor passes intentionally publish the same consumed-input occurrence. Their acceptability relies on the unchanged fresh exact-viewport request, the same native lifetime and the absence of newer invalidating input. The record is not a general parser capable of assigning arbitrary trees to windows; the production caller and producer establish that provenance.

The stored tokens neither keep a native resource alive nor confer new input permission. Inspection output precedes native finalization in the pinned integration and can exist without successful native presentation. This receipt therefore proves UI consumption within the stated scope, not presentation, admission, a completed drag, or retirement. Root route confirmation, exact pass-through enable/restore ACKs, guide/target/Center checks, physical release, resulting model state, DestroyNotify, inventory removal and quiescence retain their original authority.

## Raw batch and repeated-pass handling

The cumulative frame and pass counters used by the probe are viewport-specific in the pinned Context. `run_ui` keeps the same frame number throughout its repeated passes and increments it only after the outer call completes; cumulative pass advances at each ended pass. The probe processes events once when the frame differs, while retaining current pointer/pass diagnostics.

The pin also uses `begin_pass(new_input.take())`, whose `RawInput::take` moves the event vector out before a repeat. The frame guard is consistent with this actual physical integration. Empty passes and duplicate inspection passes cannot advance the receipt sequence or manufacture a new Move. This is not a general manual `begin_pass`/`end_pass` protocol, and no such API promise should be inferred.

The ordered event handling fixes the demonstrated observation weakness: `PointerGone` clears egui's `latest_pos()` but intentionally does not release its pointer button. Move followed by Gone in one batch or a later frame can therefore leave a valid held-motion occurrence even though the original passive hover label disappears. This explains a source-supported way to fail the old observation without proving that the particular failed native run followed that ordering.

## All call migrations and retained state checks

I counted 29 original barrier call sites and matched them to exactly 14 migrated held-motion sites plus 15 unchanged state sites. Each migrated call retains its original viewport, prior step expression, client bounds, scale, point, overall deadline and stage string. Only the fixed `true` argument moves into the combined helper, which performs the same `move_pointer` and physical pointer/button assertion.

The 14 sites cover threshold/first-target/routed-target in nested dragging; inactive Main hover; overflow entry and exit; stationary resume; drag-back threshold/first-ingress/routed-ingress; clipped Main; pane-basin motion; Shift resume; and exact Center-guide motion.

Every new baseline is read before that helper's X11 injection, with no inspection request inserted. The three moved `.max(...)` step calculations are pure expressions previously between injection and its barrier; no fallible operation or platform action was reordered with them. The original three-second bounded deadline is still derived after injection at the same semantic-loop entry, using the same overall deadline.

All 15 non-motion state calls retain their exact original argument and stage bytes, including hover, press, release, stationary held-state and Shift/no-new-motion assertions. Removing only the new motion branch, its local diagnostic slot and its extra error fields from the common loop reconstructs the entire original state-barrier body byte for byte. There is no relaxed state fallback when a held-motion receipt fails.

Failure still propagates before the existing physical cleanup release. Existing pre-cleanup diagnostics remain; motion failures additionally disclose the baseline and last typed record. No deadline increase, retry scheme, alternate target, ignored Child barrier or weakened final Center result was added.

## Tests and validation limits

The 225-line test section contains three registered tests using a real Context, real egui Plugin begin-pass callbacks, processed raw events, real UI output and the production AccessKit parser/matcher. It does not directly assign expected internal receipt fields or fabricate native identities. This gives meaningful coverage of the event-to-output boundary:

- Same-batch Move/Gone, an empty successor, rejection of using that successor as its own baseline, and a real discarded/repeated UI pass with exactly one new event-sequence increment.
- Separate-frame Gone, a stale tree sample, a baseline already containing the Move, wrong coordinates and a real foreign viewport.
- Move-before-press, later release, repress after release, and explicit focus cancellation.

The repeat test verifies the actual pinned `run_ui` path, where `RawInput::take` already drains events; it does not independently stress a hypothetical repeated raw batch outside that integration. Escape shares the cancellation branch but lacks its own explicit test event. The tests deliberately have no native token, so they do not execute native scope/lifetime replacement, window reuse, removal/recreation or the production baseline filter. Those branches were reviewed statically against the pinned APIs, not claimed as tested.

The unchanged entry compiles `assertions` under `cfg(any(target_os = "linux", test))`; this makes the new pure probe tests available to ordinary non-Linux test builds. The actual app, X11 driver and 14-site migration remain Linux-only. A successful macOS unit-test result would therefore not establish Linux compilation or real native lifetime/routing behavior. Root still needs the normal compilation/tests and unchanged real X11 scenario, including existing lifecycle and retirement coverage. None was executed in this review.

## Minimality recommendation

The latest `token` field and its separate baseline viewport check are redundant in the original package (`motion_receipt.rs:19,102`; `scenario.rs:3540`). The lifetime anchor already retains the actual viewport/context/window/create-attempt facts. A publication without the matching active native token resets the record to `None`, so `lifetime.is_some()` already excludes that case in the closed producer path. Deleting the latest-token field, its assignment and that duplicate filter condition can reduce state and debug duplication while preserving `lifetime.is_some()`, current held state, all lifetime reset logic and every matcher check. A lean variant must be rebound to its own hashes before use.

The remaining module has a concrete private purpose: one bounded latest record per actual viewport, one shared injection helper, the existing semantic parser boundary and one existing barrier loop. The retained frame/pass/latest/Gone fields explain the failure discriminator. I found no need for another event journal, receipt provider, proof script, generic validator, public authority abstraction or broader native runtime change. The optional field deletion does not change the no-blocker conclusion for this original source, and neither version is qualified by this review.
