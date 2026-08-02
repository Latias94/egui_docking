# dockspace core protocol traces

This workspace-private crate executes renderer-neutral traces directly against
`dockspace::DockEngine`. It proves the core reducer contract; it does not claim
that any UI adapter is conformant.

## Authority boundary

`dockspace.core-protocol-trace/1` has one accepted shape. The previous
renderer-intent, final pointer snapshot, route-symbol, session-symbol, and
preview-symbol shape is deliberately unsupported.

A trace may declare:

- durable workspace topology and semantic workspace commands;
- viewport lifecycle and pointer-free platform snapshots;
- complete surface measurements and actual presentation output facts;
- complete per-stream presentation observations identified by logical surface,
  endpoint role, stable stream sequence, and stable emission sequence;
- one core pointer-provider lifecycle and one or more complete ordered edge
  journal segments; and
- semantic receiver answers such as an exact tab or a dock target identified
  by surface, root, structural path, and target kind; and
- semantic Escape cancellation of the exact active click without exposing its
  core-minted session identity.

A trace cannot declare a provider lease, provider incarnation, viewport
binding, candidate ID, receipt ID, core presentation stream serial,
`HostFrameKey`, output ticket, hit-region ID, or presentation authority. Stable
presentation references are trace-local aliases assigned from actual emission
order; the harness binds them to opaque values obtained from the engine. After
a journal is submitted, it answers the exact frozen candidate roster and lets
the core validate the resulting receipts before the journal watermark
advances.

Presentation observations use an explicit exact-set batch. Every pending
stream is named by the fixture as `NoUpdate`, `CapturedUnknown`, or `Retired`;
the harness never enumerates pending streams to invent provider facts or choose
the newest emission. Expected outcomes preserve the stable stream, capture
generation, retirement watermark, presented emission, retired output count,
promotion eligibility, and every public rejection class. A separate
`interactive_surface_roster` is observed only through the public engine query.
It proves the popup-plane barrier: one surface proof cannot authorize any
surface, and the complete roster becomes interactive atomically only after all
proofs exist.

Interaction expectations preserve every public `InteractionOutcome` variant
as a stable outer result class. Pointer-edge results remain attached to their
exact edge, while semantic interaction results are attached to the exact
producer-local ingress which caused them. Cancellation reasons are not folded
into a generic failure class. Pending drag release and pending contained-
transform release are distinct trace values, while their core-minted session
and preview identities remain outside the fixture schema.

A scene-gesture continuation remains a core-minted, same-session proof that
only preserves the gesture source across its own core-owned select or raise.
It is not an interactive-surface roster proof or target authority: staging has
no preview, and a target can appear only after a fresh authoritative scene.
External workspace, policy, scene, or authority revocation clears the
continuation and is observed through the existing typed cancellation outcome.
The harness matches `InteractionOutcome` exhaustively, so a new public outcome
cannot be accepted without a corresponding trace-schema projection.

## Host-frame order

Every boundary executes in this order:

1. Apply an optional provider operation. Activation authorizes the following
   host frame. Retirement publishes its own complete transition and ends the
   boundary without creating an empty host frame.
2. For non-retirement boundaries, begin a core-minted host frame and submit its
   complete presentation observation.
3. Replay the sole `events` vector in order. A trace may group contiguous
   pointer edges in one journal event, but the harness submits one edge
   segment at a time and answers its frozen receiver candidate before the next
   edge or semantic input. The core mints one causal ordinal per edge (or one
   ordinal for an empty journal segment) and one per semantic input; the trace
   event index is only an arrival-order label.
4. Record actual presentation outputs against the post-input candidate.
5. Submit the exact post-input surface roster and contributions, then finish
   atomically.
6. Compare the full observable transition and, after the last boundary, the
   canonical workspace snapshot.

`retained` is an actual paint operation, not a cached-data hint. It calls
`record_painted_surface_contribution`, so the core pairs the frozen Ready
candidate with a core-minted presentation emission. A later complete
observation is required before that output can authorize interaction. Missing
callbacks, missing release edges, and `Unknown` authority never become inferred
facts.

Receiver fixtures name one semantic target; the harness never selects a winner
by scanning the core manifest. It does bind that declaration to the current
core output and verifies the reduced edge preserves sequence, pointer identity,
button transition, event-time location/route, capture authority, provider
incarnation, and stream incarnation. This is deliberately a transport and
causality oracle. It does not independently prove the core's hit geometry or
stacking compiler, and it is not UI-adapter conformance evidence; those require
an external receiver/geometry oracle and real adapter runners as release gates.

## Executable coverage

The checked-in fixtures cover semantic commands and MRU, native viewport
registration, pointer-free platform inventory, complete multi-surface
contributions, presentation settlement, PEJ-01/03/04/05/06/09/14, and a 1x source
to 2x target cross-window drop. The typed click traces cover exact tab-close
press/release, receiver mismatch, and semantic Escape cancellation. Negative
tests also prove that sequence gaps, watermark replay, caller-supplied
incarnations, stale-authority upgrades, contradictory delivery fields, and
adapter-forged click/session identities fail closed.

The checked-in scroll journal trace preserves device and sequence tokens,
phase, delta unit and both vector components, momentum, modifiers, delivery
endpoint, and an explicit structural receiver receipt. It executes
`Begin -> Update(Unknown receiver) -> Update(exact receiver) -> End`, proving
that `Unknown` preserves the frozen owner without applying a delta. Separate
negative cases extend PEJ-03/06 to the scroll lane: an invalid receiver symbol
never falls back to a manifest-selected winner, an active token cannot be
replaced, a rejected host frame rolls its watermark back, and a completed
token cannot be resurrected.

The checked-in provider-retirement trace proves that retirement publishes one
transition and advances exactly one reducer tick. Retirement boundaries reject
all host-frame events, presentation facts, and surface contributions before
the provider lease is consumed, so a corrected boundary remains replayable.

The typed popup-gate traces settle two independent surface streams in both
A-then-B and B-then-A order. They prove that A-only, `NoUpdate`, captured
`Unknown`, and terminal unavailable retirement never expose a partial
interactive roster. A structural splitter trace additionally begins a resize,
withdraws only the sibling surface authority, restores the exact fresh roster,
and proves that the later release cannot revive or commit the cancelled
session. Mutating either one typed presentation outcome or the final roster
makes replay fail.

## Current click-trace limits

`CancelActiveClickWithEscape` resolves a click which is already `Pressed` at
the start of the boundary. The schema still cannot name a click session armed
by an earlier journal segment in that same boundary; exposing a caller-minted
session symbol would weaken the authority contract.

`SourceVanished` has an exact, distinct cancellation projection, but no legal
click trace currently reaches that defensive release branch. Every exposed
workspace, policy, scene, surface, and provider mutation cancels `Pressed`
first with its own typed reason. A future input which can invalidate only the
frozen close source must add an executable `SourceVanished` trace rather than
fabricating one against today's reducer order.
