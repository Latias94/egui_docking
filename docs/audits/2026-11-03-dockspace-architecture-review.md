# Dockspace architecture and host-contract review

Date: 2026-11-03. Reviewed source: `536c453f80e9e86a4dbe31a1e133b42f7734d603`.
Status: review complete; the findings and proposed contract changes below are not implemented.

## Judgment and scope

Keep the renderer-neutral product and its single canonical docking forest.
Refactor the ownership of transactions, presentations and host updates. The
current implementation has strong identity and rollback machinery, but some
user operations depend on unrelated global state or renderer scheduling. These
are important limitations for a self-drawn game-engine editor.

This review uses the current code, the previous semantic-state audit, and the
local ImGui docking reference at `d88f238d1d363ef387382b2e29ad12cf96f826ac`.
The previous audit's passing qualification remains evidence for its declared
profiles; it does not cover the new counterexamples here.

Five independent read-only lanes reviewed:

| Lane | Reviewer | Scope |
| --- | --- | --- |
| Model | Ramanujan | Canonical graph, transactions, workspace publication and close dependencies |
| Interaction | Hypatia | Pointer journals, source continuity, presentation, guides, touch and decoration |
| Native | Dalton | Effects, preparation, retirement, resource obligations and degraded facts |
| Host interface | Bernoulli | Public engine integration, output lifetime, focus and persistence |
| Verification | Schrodinger | Actual examples, CI, sequence testing and performance coverage |

The main thread checked the reported call chains and ran independent public-API
probes. Reviewers ran no Cargo and made no edits. Production source, reference
checkouts, manifests, lockfiles and the existing CI/review changes are unchanged.
Open-GPUI integration and new backend certification are outside this review.

## Executed evidence

The temporary consumer depends on the production `dockspace` and the existing
public host recording renderer. It does not expose private engine modules.
Existing public focus, preference and pending-release fixtures were copied into
that consumer; the added expectations use the same public seams. No production
mutation was used to force a failure.

Final selected run: **12 tests, 5 passed and 7 failed expectations**, with 14
unselected fixture tests. Four passes are controls; the fifth confirms an
existing pending-release cancellation contract. The seven red expectations
include five implementation/UX findings and two deliberate contract limitations;
they are not seven newly discovered implementation regressions.

| Expectation | Result | Classification |
| --- | --- | --- |
| Two pending scenes with different tab measurements remain presentable | Pass | Control |
| Three emissions with identical measurements reuse a scene | Pass | Control; rules out a broader claim |
| Deferred close without an unrelated mutation completes | Pass | Control |
| Initial measurement becomes paintable after a separate commit | Pass | Control |
| A new gesture cancels a released operation awaiting presentation | Pass | Existing contract, D3 |
| Three pending scenes with different tab measurements remain presentable | Fail at the middle emission | F1 |
| Closing background A preserves confirmed focus on B | Fail; requests A's successor | F2 |
| Removing a recovery host while keeping its child preserves saveability | Fail with `UnknownRecoveryHost` | F3 |
| Expanded touch regions preserve each visible splitter center | Fail; mouse succeeds, Touch hit is unavailable | F4 |
| A visible inner guide's center selects its own operation | Fail; selects Outer | F5 |
| Unrelated B selection preserves A's deferred close | Fail with `Inert(RetiredRequest)` | D1 |
| Initial measurement is paintable before core publication | Fail; `paint_plan` is `None` | D2 |

Local artifacts:

- `/tmp/dockspace-architecture-review-current.json`: commands, baseline, probe hashes and results.
- `/tmp/dockspace-architecture-review-_5wgqj58/tests/`: public probe sources.
- `/tmp/dockspace-architecture-review-_5wgqj58/public-seams-final.log`: final run.

```text
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
CARGO_PROFILE_TEST_DEBUG=0 CARGO_TARGET_DIR=<repository>/target
cargo nextest run
  --manifest-path /tmp/dockspace-architecture-review-_5wgqj58/Cargo.toml
  --offline --test-threads=1 --no-fail-fast
  -E 'test(review_) | test(control_) | test(a_new_gesture_gets_a_new_identity_after_pending_replacement)'
```

The assignment and command above describe one invocation. These temporary paths
are local evidence, not portable checkout assets. Before remediation, move the
minimal counterexamples into maintained tests. This review did not rerun the
complete platform matrix or inject physical macOS input.

## Executed implementation and UX findings

### F1 — P1: an in-flight middle scene can be discarded before its output settles

Emit three outputs A, B and C under the same workspace requirements and coordinate
authority, changing `TabIntrinsic` from 96 to 104 to 112. Report each output as
Presented in order. A binds; B cannot bind its receiver. The two-scene control
passes. Three emissions with identical measurements also pass because
[`install_ready`](../../crates/dockspace/src/scene/lifecycle.rs:1091) reuses the
scene when its plan and coordinate capture are unchanged.

For genuinely different plans, the ready scene retains one candidate and one
fallback. [`into_retained_authority`](../../crates/dockspace/src/scene/lifecycle.rs:178)
keeps A while C displaces B. The output ledger still accepts B's affine result,
but [`retained_output_capture`](../../crates/dockspace/src/engine/presentation_authority.rs:1650)
cannot find B's scene and revokes interaction for that stream. Scene lifetime
therefore does not cover the lifetime of all admitted emissions.

This affects changing title/font measurements in a renderer with multiple
outputs in flight. The test proves unavailable receiver authority at B's
settlement; it does not measure the duration or frequency of visible stalls in
a real renderer. It is not a defect in every multi-frame or identical-measurement
path.

Minimal repair: apply explicit admission backpressure before displacing an
in-flight scene. Preferred repair: make the existing emission owner retain the
scene's shared plan/hit/semantic data until settlement and current-presentation
retention release it. Bound the retained set. Preserve requirement, coordinate,
epoch and owner revocation; retaining a scene must not revive obsolete authority.

### F2 — P2: a background close requests focus away from the confirmed winner

Select and confirm focus on A, then select and confirm focus on B. Closing A's
historical focused pane requests focus for A's next tab, despite B being the
confirmed winner. The public probe observes a new request for `Pane(ItemId(2))`
on A.

[`content_close_focus_successor`](../../crates/dockspace/src/engine/close_workflow.rs:771)
consults per-surface history when no request is pending. A successful focus
observation clears the pending request while retaining that history
([`viewport_focus.rs`](../../crates/dockspace/src/viewport_focus.rs:3674)). The
existing protection covers a newer request that is still pending, not a newer
request already confirmed on another surface.

Repair successor selection against the existing global focus winner, then use
surface history only to choose its replacement. In a further interface slice,
allow the host to report focus leaving docking for its toolbar, canvas controls
or another UI scope; a request acknowledgement alone cannot describe every real
focus transition. Keep current focus, per-surface history and pending intent as
distinct concepts within the existing focus coordinator.

### F3 — P2: valid layout reset can leave a document unsaveable

Cold-restore a saved document with Child B whose recovery host is A. No native
binding needs to be created. Reset the layout to keep B and remove A. The reset
commits and B's items remain valid, but `save_document_json` fails with
`UnknownRecoveryHost { surface: B, recovery_host: A }`.

[`reconciled_viewport_placements`](../../crates/dockspace/src/document.rs:462)
preserves a previously declared hosting preference for an unbound surface.
[`retain_surfaces`](../../crates/dockspace/src/viewport_persistence.rs:264)
removes obsolete rows without reconciling references from surviving rows. The
graph and document-side presentation preferences have different validity
boundaries. Whole-empty/reset-and-restore tests do not cover this surviving-child
case.

Minimal repair: clear an unbound hosting declaration whose recovery host no
longer exists, preserving independent placement hints. Preferred repair:
membership/reset publication must reconcile the existing document sidecar
atomically. Specify whether an orphaned preference is cleared, rebound to an
explicit successor, or causes the mutation to reject. A successful layout reset
should preserve the ability to save the resulting document.

### F4 — P2: touch expansion can disable the original visible handle

Use three 120-point panes with two 1-point vertical separators in a 362-point
surface and valid `touch_resize_hit_extent = 256`. Mouse hit testing at each
visible separator center finds its receiver. Touch returns no `HostPointerHit`.

[`presentation_hit/touch.rs`](../../crates/dockspace/src/presentation_hit/touch.rs:45)
expands parallel separator bands independently. The overlapping same-priority
records produce ambiguity in
[`resolve_exclusive_matching`](../../crates/dockspace/src/presentation_hit.rs:992),
which the public query converts to unavailable. The compiler created the
ambiguity that the resolver correctly rejects. Existing tests check expansion
and protected chrome, but not ownership of adjacent parallel handles after
expansion.

Partition expanded areas around neighboring handle midpoints and junctions,
preserving the original visible handle's region. Use the existing device hit
manifest; adding a second touch resolver would duplicate the authority.

### F5 — P2 UX: simultaneously visible inner/outer guides have no shared space allocation

A public fixture equivalent to the basic editor's full-height left column uses
a 1321-by-600 root and 20% left share, approximately 264 points. It emits:

```text
inner-left draw: x = 48.000002 .. 96.000002, y = 290 .. 338
outer-left draw: x = 48        .. 96,        y = 276 .. 324
inner-left center: approximately (72, 314)
selected explicit target at that point: Outer
```

The test checks the actual inner guide is emitted, not merely present in a
dormant structural plan. Both guide placements are valid independently:
[`inner`](../../crates/dockspace/src/scene_compiler/surface.rs:2336) and
[`outer`](../../crates/dockspace/src/scene_compiler/surface.rs:2437). Their shared
layout is not validated. Exact outer precedence and inner-before-outer painting
are intentional in the [current resolver contract](../../crates/dockspace/src/drop_resolver.rs:1659)
and match the relevant ImGui rule; this is not an inverted render order or random
target resolution.

ImGui sizes markers using font size and parent extent, places outer controls
near root edges, suppresses duplicate root-central directions, and gives explicit
outer hits priority
([geometry](../../repo-ref/imgui/imgui.cpp:20249),
[selection and painting](../../repo-ref/imgui/imgui.cpp:21794)). It also does not
guarantee universal geometric non-overlap.

Allocate space for the simultaneously active inner and outer affordances in one
core-owned pass. Each visible action needs a distinguishable selectable area;
draw and hit geometry must change together. Reducing one default size moves the
collision to another pane size. If an outer control wins an overlap, the
overridden inner cue should not remain visually ambiguous. This headless test
does not certify a physical-window resize journey.

## Product contracts to reconsider before stabilizing the engine interface

### D1 — high-priority P2 design limitation: deferred close depends on the whole workspace

A deferred save-and-close on A is invalidated by selecting a tab on unrelated B.
The later approval returns `Inert(RetiredRequest)` and A remains open. The
no-intervening-mutation control closes A normally. This behavior follows the
current authority contract and is not a missing stale check.

[`advance_revision`](../../crates/dockspace/src/engine/presentation_authority.rs:91)
invalidates close plans using global `WorkspaceVersion`. Prepared close execution
also compares a frozen whole-workspace item multiset
([`operation.rs`](../../crates/dockspace/src/operation.rs:255)). Simply ignoring
the version would leave this second dependency and could permit unsafe removal.

Use domain/epoch, exact target membership and relevant policy dependencies for
approval lifetime. Revalidate the approved removal against the current graph,
checking that only the approved item set is removed. Reject changes to the
target or its permissions while allowing unrelated selection or other roots'
independent close decisions. This removes a retry state machine from every
application implementing asynchronous save dialogs.

### D2 — P2 design limitation: updates needing new measurements require separate commits

The public initial-measurement probe gets `paint_plan == None` before committing
measurement; its two-commit control passes. Measurement already prepares a
compiled contribution, but
[`measure_surface_with`](../../crates/dockspace/src/runtime/measurement.rs:284)
queues it for later reduction. [`paint_plan`](../../crates/dockspace/src/runtime/paint.rs:1235)
reads the installed scene, and confirmation rejects an already answered surface
([`paint.rs`](../../crates/dockspace/src/runtime/paint.rs:1302)).

This internal scheduling choice becomes an external host obligation. The actual
egui adapter performs separate commits, merges reports and handles the interval
between them. The earlier blank-frame failure was repaired at that adapter, but
the interface still exposes this orchestration.

Make input, private layout compilation and output encoding phases of one
rollbackable update. An internal measured surface can then transition to an
encoded surface without being treated as a duplicate answer. The eventual
renderer Presented/Dropped observation remains later and independent. App-owned
pane callbacks must execute once per actual UI pass; core rollback must not
claim to undo or replay application side effects.

### D3 — high-priority contract decision: renderer latency can cancel a completed local gesture

An authoritative release freezes a mutation until its exact preview is
presented. A new valid press can cancel that already released operation with
`ReplacedByNewGesture`
([`pointer_transaction.rs`](../../crates/dockspace/src/engine/pointer_transaction.rs:248)).
The existing public test
[`a_new_gesture_gets_a_new_identity_after_pending_replacement`](../../integration/dockspace-host-harness/tests/manipulation_feedback.rs:540)
passes in this review and explicitly requires that cancellation.

This distinguishes a product-contract concern from an untested implementation
bug. Two quick gestures in one input batch, or a slower asynchronous renderer,
can have different user-visible outcomes than a renderer that presents between
them. Active input ownership and an accepted released intent have different
lifetimes; one slot currently handles both transitions.

Decide this before further hardening the current contract. A limited repair can
apply bounded input admission while an accepted local mutation settles, without
canceling it. The more substantial option is to commit ordinary logical local
layout intent at validated release, keeping presentation results responsible
for subsequent hit authority. Preserve the separate native cross-window/resource
transfer and exact first-live requirements. This requires intentional breaking
tests for rapid double gestures, dropped outputs, target revocation and delayed
renderers; it should not be implemented by merely deleting the existing barrier.

## Static findings requiring narrower remediation or fault injection

### S1 — P2: rollback ownership causes three full graph copies for one command

[`stage_workspace_command_with_authority`](../../crates/dockspace/src/engine.rs:5574)
clones the graph before invoking atomic `WorkspaceTransaction::apply`.
[`prepare_transaction_inner`](../../crates/dockspace/src/operation.rs:528) clones
it again. [`canonicalize_workspace`](../../crates/dockspace/src/canonical.rs:195)
clones the draft a third time. The graph contains owned SlotMap nodes, item
vectors and maps; these are not three cheap `Arc` increments.

The transaction-only clone counter omits the outer engine copy and the
canonicalizer's copy. Source tracing establishes redundant work, not a measured
frame-rate regression. Give transaction preparation one owned draft, run
canonicalization and validation inside it, and publish the resulting candidate
once. An atomic wrapper can remain for callers that do not already own a
discardable draft. Do this before considering a general change journal or another
graph representation.

### S2 — P2 protocol risk: default effect Drop claims dispatch never happened

[`NativeEffectRequest`](../../crates/dockspace/src/runtime/native_effect.rs:165)
instructs the host to execute an operation and then consume its result. Its
[`Drop`](../../crates/dockspace/src/runtime/native_effect.rs:413) queues
`DispatchFailed(ProviderStopped)` when the request is armed and still owns its
storage. Consumed terminal requests do not enqueue another failure. If a custom
host performs an external side effect and unwinds before `accepted()`, that classification is stronger
than the available knowledge. Native-create abort treats DispatchFailed as
not executed and can omit compensation
([`native_create.rs`](../../crates/dockspace/src/frame/native_create.rs:850)).

The classification path is confirmed statically. No physical native fault
injection was performed and no leaked real OS window was reproduced. Add fault
injection at side-effect-before-acknowledgement.
Separate unstarted request ownership from a dispatch guard: unstarted Drop may
mean not dispatched; dispatch-in-progress Drop must preserve indeterminate
outcome and cleanup obligations. Keep the original affine storage owner.

### S3 — P2 liveness risk: bad work-area facts suppress independent destruction facts

[`record_viewport_roster`](../../crates/egui_dockspace_native/src/coordinator.rs:2384)
requires successful window compilation and work-area freezing together. A
work-area roster returning `Err` routes otherwise valid observations and
Destroyed tombstones to inventory-Unknown fallback. The ordinary `Ok(Unknown)`
work-area result still allows window observations to be submitted. Failure during
work-area preparation instead returns an error before snapshot submission. Neither path preserves independent
terminal progress for that attempt.

Destroyed retirement cannot complete its snapshot/tombstone transition until a
later valid roster. Persistent unrelated geometry failure can retain terminal
slots. This is a confirmed conditional call path; this review performed no
physical native fault injection and measured no OS occurrence.
Degrade only the invalid fact domain to Unknown, retain valid destruction facts
in the existing joined order, and continue to treat real capacity failures as
capacity failures. No second native event coordinator is needed.

### S4 — P2 visual regression candidate: source decoration depends on current tab visibility

[`drag_decoration::resolve`](../../crates/dockspace/src/runtime/paint/drag_decoration.rs:168)
requires the source item to remain in the current visible TabRecord set. Strip
scroll recompilation can omit that record while the exact source continuation
keeps dragging alive. The egui ghost and its Grabbing cursor are emitted through
this decoration. Existing edge-scroll fixtures drag from an independent source
pane that never scrolls out of view.

This call-chain finding was not executed in the new probes. Reproduce a drag
whose own source strip scrolls the source offscreen, then use the existing full
TabStripMemberRecord or frozen source facts for decoration. Preserve current
input revocation rules; keeping source visuals alive must not keep stale hover
authority alive.

## Verification and maintainability gaps

1. **P2: actual-editor tests are not fixed-HEAD CI gates.** The maintained
   [`editor_docking_x11.py`](../../integration/egui-native-smoke/editor_docking_x11.py:29)
   and `editor_x11.py` exercise the actual basic composition. Fixed-HEAD CI runs
   a different physical-smoke application. The uncommitted external CI edit
   also does not add these drivers. Add the existing finite drivers to the
   existing X11 environment; avoid creating another journey DSL.
2. **P2: the long command model has unreachable coverage.**
   [`command_model.rs`](../../crates/dockspace/tests/command_model.rs:38) starts
   with every item open; its operation generator has no Close, so its Open
   branch cannot become reachable. Successful Results are counted without
   checking the semantic selection/order/destination postcondition. Keep
   structural invariants, add actual operation-coverage assertions, and use a
   small public state model rather than copying the docking algorithm.
3. **Full-update cost is not covered by the existing frame sample.**
   [`frame_cost.rs`](../../integration/dockspace-host-harness/examples/frame_cost.rs:58)
   samples begin, optional selection, deferred contributions and commit. It does
   not time the complete measurement/encoding/output/Presented round trip.
   Structural recording-size tests remain useful, but cannot measure newly
   introduced uninstrumented allocations. Add scoped allocation and retained
   memory samples for idle, selection, dragging and multiple in-flight scenes
   at the existing 16/128/1024 sizes. Set latency budgets from the target engine
   workload rather than making unsupported ImGui performance comparisons.

The two existing Python scripts use standard Cargo facilities. This review
found no custom compiler frontend or second docking algorithm in tooling.

## Target module responsibilities

The purpose is to reduce what a host or a maintainer must coordinate, not simply
to split large Rust files. The three-copy path demonstrates why ownership
matters more than file size.

| Existing area | Target responsibility | Consolidation/deletion |
| --- | --- | --- |
| `graph`, `workspace`, `operation`, `transaction`, `canonical` | One immutable canonical snapshot; one owned mutation draft; checked publication | Remove nested clone/apply/normalize rollback wrappers in the same operation |
| `interaction`, pointer handlers, `pending_mutation` | Current gesture, accepted terminal intent and source continuation with explicit lifetimes | Reconsider replacement cancellation; do not add a second gesture state machine |
| `PresentationAuthorityState`, scene lifecycle, emission ledger | Candidate scenes, admitted output ownership, actual presentation and receiver validity | Replace arbitrary one-fallback retention with output-owned bounded retention/backpressure |
| `DockspaceHostFrame`, measurement and paint | One input/layout/encode candidate and one core publication | Remove measurement-then-paint commit choreography from adapters |
| `CloseCoordinator`, document sidecar, focus coordinator | Scoped operation dependencies; referentially valid preferences; current focus distinct from history | Remove whole-workspace invalidation where exact relevant dependencies suffice |
| Native coordinator and egui native adapter | Core transfer semantics plus framework-specific resource facts | Isolate dispatch/retirement mechanics from eframe token conversion; validate reuse with a real second host before adding a stable helper crate |

The public engine interface can expose an explicit update phase progression
without exposing internal contribution IDs. Hosts continue supplying real
input, geometry, measurements, encoding and terminal renderer results. A result
may supply repaint, deadline and effect work together so each host does not
reconstruct progress from several unrelated polling queries. This is an
interface direction to validate, not a new scheduler authority or generic GUI
description language.

## Refactor sequence and acceptance

| Slice | Change | Required exit evidence |
| --- | --- | --- |
| R0 | Adopt minimal public counterexamples and real basic CI gate | All failures reproduce on the frozen baseline; controls retain their exact scope |
| R1 | Fix admitted-scene lifetime and bound outputs in flight | Three changing scenes, out-of-order results, Dropped, revocation, retirement and bounded retention pass |
| R2 | Specify local release intent lifetime and one-update phases | Rapid consecutive gestures cannot silently discard accepted intent; one private input/layout/encode candidate publishes once; native first-live remains exact |
| R3 | Give one transaction draft ownership; scope deferred-close dependencies | Clone accounting covers the public command path; rollback/retry and close-on-A while using B pass |
| R4 | Reconcile focus and document preferences at publication | Background close preserves B; host focus exit is expressible; every successful reset remains saveable |
| R5 | Unify visible affordance allocation and device hit partitions | Inner/outer controls remain distinguishable; Touch preserves original handles; own-strip source decoration survives scrolling |
| R6 | Tighten native dispatch and degraded-terminal progress | Side-effect/ack fault injection, invalid work area plus Destroyed, cancel, late facts and bounded terminal retention pass |
| R7 | Refresh representative host and example qualification | Complete-update cost samples, actual basic workflows, public immediate/deferred renderers and affected native profiles agree |

R1 is the immediate correctness priority. D3/R2 is the central architectural
decision: settle the intended local interaction semantics before stabilizing
more interfaces around the current pending-release behavior. R3 is a contained
performance and ownership cleanup and does not require a new transaction
framework.

Preserve the single N-ary graph, stable Item/Root/Surface identities, distinct
binding incarnations, exact receiver facts, atomic publication and native
first-live transfer. Do not copy ImGui's global mutable context, add another
lifecycle registry, or weaken stale/foreign proof checks to make tests pass.
The work is complete when these product contracts and real host journeys agree;
a larger passing test count alone is not the acceptance criterion.
