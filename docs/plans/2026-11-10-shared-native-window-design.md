# Shared native window design

Status: **implemented and qualified for the recorded finite Linux X11 profile**.
Design baseline: `6a63fb6`. Complete-journey implementation baseline: `78c1eaa`.
Core and public-host implementation: `270d81b`; native adapter and real-window
consumer: `3136d5e`. The public composition and
window-scoped lifecycle pass all sixteen serial gates and eighteen real-X11
gates, including the shared-window journey and its late-discard variant. The
[qualification record](../audits/2026-11-11-shared-native-window.md) maps each
acceptance step to public-host, native component and physical evidence. The
physical profile uses one screen with native DPI one and independently scaled
surface placements; it does not qualify OS DPI transitions, other backends or
new performance comparisons.

The [implementation plan](2026-11-10-shared-native-window-implementation.md)
records the earlier internal ownership slice and completed G01 phase. Separate
G02/G03 commands and G04 external-Root batch remapping retain their own evidence;
the new shared-window gates qualify this composition directly.

## Scope and completion rule

Support one real external Root window containing multiple independent logical
docking surfaces, with dragging between those surfaces and runtime-owned Child
windows. Preserve exact input, presentation, creation, close, recovery and
retirement authority. Ship these behaviors together; do not expose placements
while retaining surface-based window close, vacancy or first-live decisions.

The initial shared composition is limited to externally owned Root windows.
Runtime-owned Child windows retain one whole-client surface placement and their
existing create/recovery obligations. A surface has at most one active native
placement. This is not a general window manager or render graph. The host owns
OS handles, layout choices and framework composition. Core owns docking state
and the validity of the facts used to change that state.

## Identity and placement

`NativeWindowBinding` identifies one real OS-window incarnation and contains no
`SurfaceId`. It retains provider, engine-domain, workspace-epoch, token and
incarnation checks. Root/Child role, ownership, capture, physical focus, native
preparation, close and retirement belong to that window.

`NativeSurfacePlacement::new(surface, size, origin, scale_factor, clip)` describes
one logical surface's host-chosen projection. `size` is the independent logical
layout size; `origin` and `clip` use physical window-client coordinates, and
`scale_factor` is physical pixels per surface logical unit. Clipping does not
resize the layout or constrain captured source movement. Submission validates
positive sizes, finite transformed extents and the complete roster.

The description itself grants no authority. Core associates it with an exact
window and mints a placement generation in `NativeSurfaceBinding`. That binding
identifies the surface placement; its `.window()` returns the separate
`NativeWindowBinding`. Use the latter for physical facts and resource ownership.
`same_window_lifetime` supports lifetime correlation across provider handoff;
it does not authorize a superseded provider to report facts. Contained roots
remain floating roots inside a surface, not substitute surfaces.

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

## Public host and adapter migration

These are the implemented entry points covered by the qualification record:

| Boundary | Migration contract |
| --- | --- |
| Complete Root placement roster | Enroll the existing native host and register the real Root once. Call `DockspaceHostFrame::set_native_surface_placements(window, placements)` before measurement/encoding to replace its complete nonempty roster atomically. Entries must name unique existing logical surfaces. An empty roster is rejected; it is not a request to retain a window without any surface. |
| Candidate and committed queries | `native_surface_bindings_for_window` and `native_surface_placement` are available on the frame and session. Frame queries expose the private candidate; session queries expose committed exact bindings. `DockspaceSession::native_window_bindings` enumerates real windows once each. |
| Reference Root UI | Call `NativeDockspaceApp::show_root_surfaces(ui, placements)` once from the Root UI callback. The complete roster must contain `root_surface()`, and repeated passes must supply the same roster. The ordinary app callback starts as the single-surface convenience path; after composition it honors the admitted or retained complete Root roster. Child callbacks remain whole-client. |
| Physical observations and input | Snapshot entries describe real windows once, rather than repeating facts for sibling surfaces. `NativeGlobalFocus::Dock`, `NativePointerHover::Dock` and `NativePointerOwner::Native` now take `NativeWindowBinding`. Use `binding.window()` when migrating an existing surface handle. Duplicate sibling entries for one physical window are rejected. |
| Event-time client geometry | `NativeLocalGeometry::new(content_size, native_scale_factor)` describes physical client geometry. Obtain local coordinate authority for the window; the surface's placement scale is applied later by core. Keep the original event geometry and avoid applying native DPI a second time. |
| Receiver callback | `NativeReceiverQuery` is now an enum. Answer `WindowHit` from the completed framework hit graph with `NativeReceiverAnswer::WindowHit`; only its `Surface` variant exposes the surface-qualified receiver question. |
| Renderer terminal | `report_surface_presentations(outputs, result)` validates and settles a complete owned collection. On error, `SurfacePresentationBatchReportError::into_outputs` returns every original affine output in order. The single-output method remains available for independently owned outputs. |

An identical complete placement roster is a no-op, including while a native
close request or its cancellation is pending. Actual membership or geometry
changes remain refused while that close scope is owned. Do not split a roster
update into a sequence of independent surface commits or fabricate window tokens
for sibling surfaces.

Native raw input is admitted against a complete roster before the application UI
callback runs. If that callback declares different placements, the input-owning
run paints its frozen admitted roster; the new declaration is eligible on a
later run after its own full capacity check. Switching to the ordinary Root
callback does not truncate the frozen roster. Original Text and IME events are
neither copied nor replayed. The adapter retains one pending placement request
and one admitted-run snapshot, charges their backing to its existing storage
allowance and preserves the original snapshots on capacity refusal.

Dockspace product menus inherit the source placement scale while using the
whole viewport for popup constraints. Their hit and accessibility transforms
match their output proof. A pane's independently created framework popup keeps
egui's ordinary viewport-point overlay semantics.

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

For `NativeReceiverQuery::WindowHit`, the receiver adapter supplies a
`NativeWindowHit`: `Surface` with the exact presented output artifact,
`WindowBackground`, `Blocked` or `Unknown`. The query point is physical client
space. Core validates the selected artifact, window, placement, generation and
clip before invoking `NativeReceiverQuery::Surface`. That second query carries
surface-local logical coordinates and the existing click/drag/scroll/hover
purpose. Do not select a surface from a retained source label or geometry alone.
Overlays and undeclared framework widgets remain blockers; window background
does not prove desktop outside-all.

Desktop native hover remains an independent OS fact. Foreign and Unknown never
become clear merely because a registered window contains the point. Preserve the
existing authoritative release and outside-all requirements.

Local-to-desktop handoff carries both observations of the same event. Core checks
physical-point equality, source continuity, delivery/capture, generations and
placement conversion before upgrading the existing stream. Failed upgrade keeps
the valid local fallback. Never submit the event through a second producer.

Touch, long press and scroll use the same placement proof. Locked scroll retains
its existing receiver lifecycle. `NativeGlobalFocus::Dock(window)` reports only
physical window focus. It does not automatically activate a sibling surface or
select a pane; surface/pane focus still requires its own exact framework
observation.

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

Validate every contribution's proof and span order before confirming any
contribution, then publish the existing core frame once. The collection helper
`egui_dockspace::native_support::validate_final_outputs` validates the complete
set before removing its private proof markers once. A failed member leaves all
other members unscrubbed. A renderer terminal settles the complete owned
collection as Presented or Dropped. Receiver promotion still waits for the
corresponding core presentation commit. Retained artifacts do not receive
fabricated new presentation credit.
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

One native close request targets one window. Core freezes its complete exact
surface/content roster and collects approval through one close plan and one
transaction. Defer or veto must not partially close siblings. Actual placement
changes cannot replace that owned scope; a stale close cannot absorb a newly
added area. Content permission can be revoked without dropping an already
emitted effect's exact acknowledgement and destruction obligations.

Removing a placement is distinct from destroying its window. Window registry and
close/recovery owners do not depend on framework receiver storage. Destruction
is observed once for the physical window and applied to every exact hosted
placement. Each retired placement's callback and output obligations must drain
before the window lifetime can be reclaimed. A late physical callback retains
its exact effect correlation; it cannot affect a successor using the same token.

External Root continuation retains the physical window while replacing its exact
logical epoch and placement scope. Prepare the whole continuation roster, publish
it atomically, and install host routes only from the matching committed report.
Old callbacks cannot mutate successor placements. Physical focus may follow a
proven continuation; pane focus, coordinates, receivers and output authority need
fresh evidence. Recovery availability must be checked before dropping old scope.

## Public acceptance journey

These journeys define the completion gates for the implemented entry points.
The qualification record distinguishes public protocol coverage from actual
physical execution rather than claiming every negative gate was injected into
the window manager.

1. Enroll one Joined host, register Root W once, then atomically place independent
   surfaces A and B with independent layout sizes, origins, scales and clips.
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

## Completion and qualification order

1. Complete the public acceptance fixtures and exact negative gates against the
   window/placement identity, coordinate units, first-live roster and close scope.
2. Verify registry, physical ingress, endpoints, vacancy, preparation, close,
   recovery and Root continuation as one coherent lifecycle.
3. Verify Joined source/hover routing and same-event handoff together with current
   interruption, focus, scroll and touch lifecycles.
4. Verify native output collections, finalizers, receiver promotion and the egui
   driver, including discarded passes, affine drop and finite backpressure.
5. Complete fresh serial consumer gates and the real X11 shared-window journey.
   Update support status only from the recorded platform evidence.

An internal checkpoint or a successful recording-host test does not complete G01.
Programmatic item/group/root tear-off, default placement and batch Root
continuation are independent capabilities. Shared-window support must preserve
their existing payload admission and transaction owners throughout this migration.
