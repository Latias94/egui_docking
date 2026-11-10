# Shared native window design

Status: **proposed, not implemented**. Design baseline: `6a63fb6`.
This document specifies G01. Separate G02/G03 commands and G04 external-Root
batch remapping do not establish shared-window support. No platform or physical
interaction support is claimed by this proposal.

## Scope and completion rule

Support one real external Root window containing multiple independent logical
docking surfaces, with dragging between those surfaces and runtime-owned Child
windows. Preserve exact input, presentation, creation, close, recovery and
retirement authority. Ship these behaviors together; do not expose placements
while retaining surface-based window close, vacancy or first-live decisions.

This is not a general window manager or render graph. Initial placement has only
an origin, a positive uniform scale and an axis-aligned clip. The host owns OS
handles, layout choices and framework composition. Core owns docking state and
the validity of the facts used to change that state.

## Identity and placement

`NativeWindowBinding` identifies one real OS-window incarnation and contains no
`SurfaceId`. It retains provider, engine-domain, workspace-epoch, token and
incarnation checks. Root/Child role, ownership, capture, physical focus, native
preparation, close and retirement belong to that window.

`SurfacePlacement` associates one `SurfaceId` with one exact window binding.
It carries origin, scale and clip plus a core-minted placement generation.
Initially a surface has at most one active native placement; a window can host
many surfaces. A Child with one whole-client placement is the ordinary case.
Contained roots remain floating roots inside a surface, not substitute surfaces.

Use one coordinate convention:

```text
client_physical = origin_physical + surface_logical * placement_scale
surface_logical = (client_physical - origin_physical) / placement_scale
```

Origin and clip use window-client physical coordinates. Scale means physical
pixels per surface logical unit. Apply clip in client coordinates. Validate
finite endpoints and extents, positive scale, and finite transformed results.
Do not multiply native DPI into this conversion a second time.

Window coordinate generation describes committed client geometry and DPI.
Placement generation describes the surface mapping. Window geometry changes
invalidate dependent coordinate proofs; changing B's placement does not change
A's placement generation. Publish placement changes only with their candidate.
A failed commit or abandoned frame must leave the committed mapping intact.

## One joined input order

Keep one Joined producer for the host. Delivery and capture identify real windows.
Local samples retain the original physical client point and event-time geometry;
they do not select a surface or authorize desktop hover. Event/callback geometry,
provider, window incarnation and coordinate generation remain exact checks.

Resolve two independent lanes from the same edge:

- Source manipulation: core retains the accepted source surface and presented
  placement proof. Captured movement may leave its clip; the host does not report
  a new source or recreate the gesture when the pointer crosses an area.
- Hover target: resolve the actually exposed placement in the hovered window,
  check its clip and exact presented artifact, then resolve its docking receiver.
  A target may have a different origin or scale from the source.

The receiver adapter supplies a narrow window-to-surface hit fact: an exact
presented placement, window background, Blocked or Unknown. Core validates that
fact before invoking the existing surface-qualified receiver protocol. Geometry
alone does not prove clearance. Overlays and undeclared framework widgets remain
blockers; window background does not prove desktop outside-all.

Desktop native hover remains an independent OS fact. Foreign and Unknown never
become clear merely because a registered window contains the point. Preserve the
existing authoritative release and outside-all requirements.

Local-to-desktop handoff carries both observations of the same event. Core checks
physical-point equality, source continuity, delivery/capture, generations and
placement conversion before upgrading the existing stream. Failed upgrade keeps
the valid local fallback. Never submit the event through a second producer.

Touch, long press and scroll use the same placement proof. Locked scroll retains
its existing receiver lifecycle. A physical window focus fact does not select a
pane: surface/pane focus still requires its own exact framework observation.

## Presentation and first live

Keep surface-local measurement, scene compilation, programs, semantic tickets,
streams, affine outputs and presentation ledgers. A native presentation endpoint
names the exact surface placement, not only its window. Interaction equivalence
must include placement identity and the required window coordinate generation.
An old artifact cannot authorize input at a newly moved or rescaled placement.

Window Prepared proves renderer/window readiness, not semantic first live. For
creation or recovery, core retains the exact required surface first-live roster:
surface, window/placement binding and a fresh semantic-output floor. Every
required contribution must pass the existing readiness and presentation checks
before the transaction transfers source ownership. A sibling's output, duplicate
contribution or earlier preparation paint cannot satisfy another roster entry.

Revoke readiness through the existing committed barrier. Late Prepared, terminal
or first-live evidence cannot cross a window incarnation, replacement, provider
or placement lifetime. Failure and cancellation retain or compensate the exact
source once. Do not replace this with adapter-owned transfer bookkeeping.

Window vacancy is computed across all hosted surfaces and outstanding resource
obligations. Moving one area's last root must not hide, pass through or destroy a
window with a nonempty sibling. Preserve the external Root treatment rule.

## Reference adapter output ownership

One eframe output token owns the actual surface contributions of that native run.
Replace the single painted output and receiver set with an owned collection.
All contributions use the same exact window, token and final completed pass;
each retains its own artifact, placement and receiver identities.

Confirm contributions at the final pass and publish the existing core frame
once. A renderer terminal settles the complete owned collection as Presented or
Dropped. Receiver promotion still waits for the corresponding core presentation
commit. Retained artifacts do not receive fabricated new presentation credit.
Repeated passes replace the complete staged contribution set; restart, early
return, renderer failure and token abandonment drop its affine outputs exactly.
Do not combine artifacts from different discarded passes or window incarnations.

Adapt egui paint, widget hit testing, pane callbacks, focus observations and scroll
conversion through the same placement transform and clip. Surface-local geometry
must not accidentally become window-global geometry. Use per-surface widget IDs
inside one viewport, without creating synthetic viewports or window tokens.

Prepay bounded contribution storage before accepting the run. Preserve stream,
output and in-flight backpressure; more surfaces do not imply unlimited native
tokens. Keep existing renderer retention and preparation owners.

## Unified close, recovery and Root continuation

One native close request targets one window. Core freezes its complete hosted
surface/content roster and collects approval as one transaction. Defer or veto
must not partially close siblings. Membership changes invalidate the captured
scope as required; a stale close cannot absorb a newly added area.

Removing a placement is distinct from destroying its window. Window registry and
close/recovery owners do not depend on framework receiver storage. Quiescence
includes all hosted contributions, pending callbacks and resource obligations.

External Root continuation retains the physical window while replacing its exact
logical epoch and placement scope. Prepare the whole continuation roster, publish
it atomically, and install host routes only from the matching committed report.
Old callbacks cannot mutate successor placements. Physical focus may follow a
proven continuation; pane focus, coordinates, receivers and output authority need
fresh evidence. Recovery availability must be checked before dropping old scope.

## Public acceptance journey

The following calls describe proposed behavior, not currently available methods.

1. Enroll one Joined host, register Root W once, then atomically place independent
   surfaces A and B with different origins, scales and clips.
2. Measure both in surface-local coordinates; paint both under one native token.
   One renderer Presented plus core commit admits their exact receiver sets.
3. Press in A, keep W capture, move to B and release. Assert one stream, source A,
   target B, correct scale conversion and one topology change.
4. Drag from A to proven outside-all and create Child C. Before its complete
   first-live roster, source stays A. Drop/retry must preserve that ownership.
5. Drag C back into B across different scales, including same-event handoff.
   Settle the move once and retire C only after window-wide vacancy/quiescence.
6. Change B placement, change W DPI, then continue the external Root through a
   workspace replacement. Exercise candidate drop and exact retry before fresh
   outputs restore interaction; stale proofs cannot target new coordinates.
7. Close W with A and B populated. Test Allow, Defer and Veto as one transaction,
   exact recovery anchors and late callbacks without partial sibling closure.

Negative gates: wrong provider/window/epoch/placement generation; duplicate or
missing first-live entries; clip-external hover with valid source capture;
Foreign/Unknown/overlay obstruction; stale or Dropped output; multipass restart;
late close and renderer terminals; finite-capacity refusal; unavailable recovery.
All must preserve ownership and allow the existing valid cancellation/retry path.

## Complete migration order

1. Add the public acceptance fixture and exact negative gates. Settle window and
   placement identity, coordinate units, first-live roster and close scope first.
2. Replace surface-bound window registry, facts and endpoints together with
   window-wide vacancy, preparation, close, recovery and Root continuation.
3. Migrate Joined input proof, receiver selection and handoff; retain strict
   source/hover separation and current interruption/scroll/touch lifecycles.
4. Migrate native output collections, finalizers, receiver promotion and the egui
   driver. Remove superseded one-to-one assumptions rather than layering them.
5. Run public consumers and focused failure/capacity regressions, then appropriate
   real-window checks. Document support only from fresh platform evidence.

Do not publish a partial shared-window interface between these steps.
Programmatic item/group/root tear-off, default placement and batch Root
continuation are independent capabilities. Shared-window support must preserve
their existing payload admission and transaction owners throughout this migration.
