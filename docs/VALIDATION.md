# Dockspace behavior-spine validation

The canonical plan was revised on 2026-10-27 to prioritize `dockspace` as a
custom-engine library and egui as its interactive validation adapter. The
external public host consumer now lives in `integration/dockspace-host-harness/`.
Its recording-renderer results prove API and output-contract portability,
not real GPU or OS presentation. Open-GPUI integration remains deferred.

This document records current qualification and the historical U1 validation
baseline. Dated qualifications supersede the historical baseline below. It keeps
semantic journeys without turning the shared conformance crate into a GUI DSL.
Executors resolve their own current semantic bounds and platform facts. The
shared Rust catalog carries the finite L03 source, basin, normalized inset
samples, cadences, and normalized outcomes alongside the existing L08 semantic
steps. It does not carry screen coordinates, guide selectors, frame counts, or
another product authority.

## 2026-10-28 U7 first bounded-cursor prefix checkpoint (in progress)

The native coordinator now claims up to 256 consecutive cursor records on one
exact device/window/viewport/binding/route lane. Active gestures keep every
motion edge. Idle coalescing remains before semantic sequencing and is disabled
when either core gesture or adapter button/contact state is active. Focus,
output, capture/route changes, contact, scroll phase and lifecycle records stop
the cursor prefix; they retain their existing causal barrier semantics.

`CursorPrefixClaim` borrows the existing mailbox mutex. Membership cannot change
while a candidate translator builds a batch and the core recorder accepts it.
A translation or recording failure drops the claim without changing the raw
queue or published translator. Once accepted, the raw payload is removed and
only the existing recorder owns replay. A dropped host frame retains that exact
semantic prefix; the mailbox commit barrier prevents later callbacks joining it.
There is no new throughput queue or gesture terminal owner.

The old `reduce_callback_head` entry is deleted, with driver, shutdown and test
callers migrated to `reduce_callback_prefix`. Its optional nonzero progress value
reports the actual raw prefix count; barrier callbacks report one. The former
universal one-callback/one-frame assertion now specifically verifies distinct
focus barriers. The existing active-core-gesture test checks count=2, preserving
its no-coalescing contract while both records share one commit boundary.

The recorder's new `batch_prefix_after` clones only the selected complete records.
It uses the actual last selected ordinal, exact predecessor append identity,
retired watermark and existing liveness/commit guard. `batch_after` delegates to
the same implementation for the existing complete-suffix contract. Four core
regressions cover 257 records as 256+1, non-consuming identical retry, empty and
retired boundaries, failed publication and rollback/reused-ordinal rejection.

Evidence was collected before behavior changes. Both native prefix tests failed
against the old implementation with the queue at ordinal 3 instead of 258 or 6.
All four core prefix tests failed on the old complete suffix (257 instead of 256,
or two instead of one). The final native tests also cover unretained-claim drop,
foreign-authority failure without queue/translator publication and exact replay
after a dropped frame. A real active-drag regression compares all 256 event-time
receiver-query points and their order on both abandoned and committed attempts;
queue counts alone are not treated as proof of semantic content.

An independent static review found no concrete defect. It identified that the
initial count/queue tests alone did not prove semantic-edge content; the active
drag point-by-point replay regression closes that evidence gap. Static review
is not counted as a test run or physical input qualification.

This is an incremental U7 checkpoint. Runtime replay still consumes its complete
recorder suffix, and the non-cursor callback families still use their existing
single-record barriers. The 4,096-record/4-MiB/250-ms admission limits, reserved
overload terminal, cooperative 2-ms reduction budget, bounded output prefixes,
1,000-Hz release-latency profile, local-touch coordinates and validated scope
continuity remain required U7 work. No whole-U7 or physical-platform completion
claim follows from this checkpoint.

Fresh checkpoint gates:

| Gate | Result |
| --- | --- |
| Root workspace, all features and all targets | 1,616 passed |
| Core backend-ingress regressions | 26 passed, including the four new prefix cases |
| Fork-backed native workspace, all targets | 203 passed |
| Native library with debug assertions disabled | 178 passed |
| Active drag: 256 receiver points, abandoned attempt and identical retry | Passed |
| Root/native correctness and suspicious Clippy groups | Passed with both groups denied |
| Strict core/adapter rustdoc, workspace formatting and diff whitespace | Passed |

Existing dead-code/style warnings remain outside these denied lint groups. The
native results are deterministic adapter evidence, not new OS input certification.

## 2026-10-28 U12 core host readiness qualification

The renderer-neutral local/contained host contract is qualified independently of
egui and native platform hosting. This completes U12's core host readiness
checkpoint. U7/U8/U10/U11/U9 and the overall docking/multiview plan remain
separate unfinished work. The application still owns UI/rendering, focus
execution, input facts and output disposition.

`integration/dockspace-host-harness/examples/engine_host.rs` is a runnable complete
walkthrough. It consumes the borrowed surface program into an owned recording,
ends the frame borrow, processes the retained records, and settles two logical
surfaces as Presented and Dropped. It does not grant receiver authority before
successful settlement. It executes an exact application focus request after
presentation, submits its observation, completes a root close through per-item
approval, and deduplicates a real cancellation by publication identity.

`host_coordinate_domains.rs` completes AE14 through eight combinations of
Immediate/Deferred consumption, window DPI 1.25/2 and framebuffer densities
2/0.75 or 0.75/2. A captured source route may lie outside A while the independently
mapped hover route lies inside B. The hover point comes from actual pane content
and misses all guide receivers; the host proves clearance and core chooses the
magnetic target. Each complete input batch waits for its exact preview output,
then publishes one changed result and one Delivered(Dock), with the same topology
and quiet subsequent settlement. Public versions remain opaque; exact internal
revision counts are covered by the core regression. A second independent static
review found no additional issue in this fixture. This is synthetic host-mapping
evidence, with explicit scale domains, rather than real platform DPI certification.

The independent Cargo workspace uses only public `dockspace` and application
record types. Its 39 default / 40 serde tests cover owned/deferred output, two
logical surfaces with equal local coordinates, complete input batches, stale and
foreign authority, rollback/retry, full contained content, commands, close,
feedback and structural output cost. The separate `dockspace_host_conformance`
crate is also a public API client: it supplies the complete 60-case L03 matrix
through immediate program acceptance. Its different crate boundary grants no
access to core-private APIs. The egui executor separately covers all four cadences,
including Burst, and 240 actual-content widget cases. These evidence sets are
complementary; the recording harness does not claim physical rendering.

Packaging was tested against the emitted artifact rather than another workspace
path. `cargo package -p dockspace --all-features --locked --allow-dirty` packaged
and verified the core. The archive was extracted to a temporary directory, and a
copied independent consumer depended exclusively on that extracted crate. Its
39 default / 40 serde tests and runnable host example passed with `--locked`.
The archive SHA-256 is
`bf821da1642e07f9a2ead8e23a526f890fd7abb17e90da110bf4a4f7a344af66`.
All 164 packaged Rust source files and the README were byte-compared with the
current crate and matched. It contains the U6 core source plus the U12 README
update; the packaging dirty flag records that documentation change, and no
registry publication occurred.

`check_core_dependencies.py` inspects Cargo's normal and build dependency graph
with all features and targets. It rejects the named UI/platform framework
families without parsing Rust sources. An independent review caught the initial
normal-only check; the corrected guard was verified on an isolated copied core
with an actual egui build-dependency and build script. It returned exit code 1
and identified egui/emath/epaint/accesskit. The real consumer graph passed.
CI now runs this guard, the host example, and every default/serde product target,
including the new feedback binary.

The public structural fixture uses balanced pane trees. It counts public work
and retained recording storage, including spare Vec capacity:

| Panes | Measurement requests | Program steps | Owned records | Record capacity | Retained recording bytes |
| --- | --- | --- | --- | --- | --- |
| 16 | 49 | 126 | 242 | 256 | 63,512 |
| 128 | 385 | 1,006 | 1,906 | 2,048 | 507,928 |
| 1,024 | 3,073 | 8,134 | 15,306 | 16,384 | 4,063,256 |

A `needs_drop` assertion prevents ordinary nested owned allocations from silently
escaping this recording-buffer count. The fixture complements the fresh U4 core
compilation counters at the same pane counts and the resolver/selection work
regressions. It does not measure total core allocations, GPU resources, wall-clock
latency or native ingress budgets.

| Gate | Result |
| --- | --- |
| Verified core archive and extracted downstream default / serde | Passed / 39 / 40 tests |
| Extracted downstream runnable engine host | Passed |
| Real dependency graph / isolated egui build-dependency negative case | Passed / rejected with exit 1 |
| Public output structural fixture, 16 / 128 / 1,024 panes | Passed |
| AE14 immediate/deferred, independent DPI/density, eight combinations | Passed |
| Adapter package contents | Listed successfully; not a registry dependency verification claim |
| Host formatting and diff whitespace | Passed |
| Strict docs / doctests after the U12 README update | Passed / 10 passed |
| Core/public/product/official/native regression | The U6 matrix in the next section uses identical core Rust source |

Historical U1 known-invalid Burst and incomplete contained rows below are retained
as audit provenance. U3/U4/U5 qualifications supersede those local blockers.
Native physical qualification is still open and is never inferred from these
recording, package, model or headless egui gates.

## 2026-10-28 U6 commands, application close, feedback and focus

Contained-window close now requests a complete root close plan. Pointer, Enter,
Space and AccessKit use the same close path; the menu's separate `Close Group`
uses the same application approval workflow. `Dock Back` remains a distinct
content-preserving command. Protected groups disable close while leaving an
otherwise valid Dock Back available. The obsolete contained-close-as-Dock-Back
frozen action, input variant and terminal helper are deleted.

The public engine consumer and egui facade cover Allow, Veto, deferred Allow,
replayed initial/continuation tokens and group atomicity. Both packaged eframe
examples actually consume permitted close decisions. Their tests invoke the
real `App::ui` callback and accessible close control; the persistence example
retains application-owned pane data and restores it from its prior document.
These tests are included as example test targets rather than merely compiling
example files.

Committed interaction events now project typed pending and terminal feedback
with one opaque manipulation identity and a distinct stable publication ID.
Motion produces no status announcements. Pending drag and contained releases
retain their original identity on presentation loss, policy change, document
restore, replacement or provider retirement. Retirement returns the actual
committed cancellation rather than manufacturing a later event. Splitter
release covers both successful delivery and consumed input-authority rejection,
followed by another successful gesture and quiet settlement.

An egui shown response forwards its committed reports and retirements in order.
Direct commands, close decisions, policy changes and restore expose their own
feedback in `DockspaceMutation::manipulation_feedback()`. The application consumes
that result at the call boundary; a later shown response does not replay it.
This fixes the independently reviewed Pending -> programmatic Dock Back/close
feedback loss without introducing a deferred forwarding queue or new identities.

Focus requests now distinguish a pane from its remaining surface workspace.
Focused item close selects the next visual tab, or the previous tab at the end,
and updates durable MRU. This intentionally differs from historical GPUI
follow-up-close selection and is recorded in the OGC-02 catalog. Closing an
unfocused target preserves a newer intent. A group close can restore a surviving
observed destination; otherwise it requests the remaining workspace. Closing a
whole standalone surface does not request focus on its retired workspace.
Pending, vetoed and rejected close decisions do not change focus.

Explicit selection publishes a pane intent even for an already selected item.
Dock/Float resolve the destination after mutation; root-level commands consider
only a currently selected pane, so a hidden historical target cannot receive an
unfulfillable request. Exact NotFocused/Focused observations exercise the public
focus lifecycle. egui registers a persistent workspace focus target and settles
it through the existing output boundary. Escape still follows egui's native
focus-clearing behavior; cancellation feedback does not request a substitute.

Independent review identified hidden-history focus, direct-command feedback loss,
a vacuous old close-as-Dock-Back assertion, missing cross-surface focus assertions
and a large close-test block. Fixes and focused regressions address these; the six
new product close tests live in a child module with their original assertions.
The review also identified pre-existing programmatic Select focus behavior, which
is completed here as part of the common command loop rather than classified as a
new regression. This is model, public-consumer and headless egui evidence. It does
not certify physical native input, GPU presentation or overall plan completion.

Fresh verification on the final U6 source state:

| Gate | Result |
| --- | --- |
| Root workspace, all features and all targets (including both example tests) | 1,612 passed |
| Public host U6 contract targets, default / serde | 37 / 38 passed |
| egui product, default / all features | 104 / 108 passed |
| Official egui, all features and all targets | 15 passed |
| Fork-backed native workspace, all targets | 198 passed |
| Strict rustdoc / doctests | Passed / 10 passed |
| Correctness and suspicious Clippy groups | Passed with both groups denied |
| Workspace/harness formatting and diff whitespace | Passed |

The public U6 counts name the four contract binaries (`public_contract`,
`contained_projection`, `close_commands`, `manipulation_feedback`); U12 output-cost
qualification is separate. Existing style/pedantic Clippy warnings are not a
clean-lint claim. The original review receipt preserves the pre-fix snapshot;
follow-up evidence records the implemented fixes and fresh gates instead of
rewriting that review history.

## 2026-10-28 U5 hover-port and composed-content qualification

The common host port uses the existing output-bound candidate, clearance,
blocked, no-receiver and unknown answers. U5 does not add a second target solver
or make egui widget ancestry a core responsibility. Official egui applications
declare ordinary content identities; popup, modal, foreign-layer, unknown-widget,
unsupported-coordinate and stale-pass cases remain conservative boundaries.

New qualification evidence:

- The product harness runs real `Button`, `TextEdit`, `ScrollArea` and canvas
  responses through all five magnetic basins, three non-guide samples and four
  existing cadences: 240 combinations. Every input sample lies inside the actual
  child response, not pane whitespace. The complete Burst gesture stays in one
  input batch. Before release there is no durable mutation; the exact final
  topology occupies one changed frame and reaches stable quiescence. This
  frame-level counter does not claim to count transactions within an egui run.
- Separate ordinary-input journeys verify button click, text editing, wheel
  scrolling and canvas drag ownership. Docking hover neither clicks the child
  widget nor converts it into a docking handle. Unknown content and overlapping
  undeclared widgets remain blockers across 24 widget/sample combinations;
  duplicating a declared ID cannot erase another widget with identical bounds.
- The official harness creates real `Popup`, `Modal`, foreground `Area` and
  same-layer explicit blockers and checks their actual bounds. It rejects both
  initial hover and terminal release through them, including a preview acquired
  before the blocker appeared. A completed unrelated egui pass cannot supply
  widget proof to an older accepted Dockspace output.
- The public-only host consumer submits current routes with old candidate,
  clearance, no-receiver and blocked facts after a new emission or scene. None
  can borrow the current output. A valid route with unknown release facts clears
  a previously presented preview without docking or floating fallback; later
  pointer selection and semantic commands still work in the same session.

Independent review found no actionable defect, but identified that the product
matrix's changed-frame counter cannot alone prove transaction-level exactly-once
delivery. The existing core
`move_then_release_with_new_preview_waits_for_exact_presentation` regression now
also checks exactly one durable revision and one `Delivered` event on settlement,
then two later settlement frames with no new revision or delivery event. This
complements the public host's duplicate-release and dropped-frame retry journey;
the product matrix retains its narrower observable frame-level claim.

The front-contained public journey exposed a real kernel defect: a background
magnetic acquisition could remain latched when the pointer entered a different
front occlusion layer near the new basin's hysteresis margin. The old target's
larger geometric envelope was incorrectly sufficient to retain it. Acquisition
and observation now carry the core `SceneLayerKey`; a different front layer or
an outside-surface observation always accepts the newly resolved result instead
of retaining the old target. Source-only scene-lineage rewriting preserves that
layer. Same-layer hysteresis is unchanged. The regression first reproduced the
background root as active, then passed with the front candidate explicitly
`Rejected`, no preview, no commit and no contained fallback after the fix.

Fixture qualification did not relax the production boundary: a full-size text
editor uses a fitting minimum row count rather than overflowing a small pane,
and non-guide samples are checked against actual guide rectangles. A target
preview after source removal is not compared to an obsolete pre-removal layout.

These are deterministic kernel, public-consumer and headless composed-UI tests,
not physical input, GPU presentation or native multiview certification. U5's
common port and official boundary do not certify the fork-backed final-stack
extension; that remains U7/U8. User-visible typed manipulation feedback, Close
Group versus Dock Back, close consumption in examples and focus successors
remain U6 work. U12 readiness and the overall plan are not declared complete.

Fresh verification on the U5 source state:

| Gate | Result |
| --- | --- |
| Root workspace, all features and all targets | 1,597 passed |
| Public host, default / serde | 30 / 30 passed |
| egui product, default / all features | 96 / 100 passed |
| Official egui harness, all features and all targets | 15 passed |
| Fork-backed native workspace, all targets | 198 passed |
| Strict rustdoc / doctests | Passed / 10 passed |
| Correctness and suspicious Clippy groups | Passed with both groups denied |
| Workspace/harness formatting and diff whitespace | Passed |

Clippy still reports existing style/pedantic warnings. The native count remains
deterministic regression evidence rather than a new physical-platform claim.

## 2026-10-28 U4 complete contained content projection

Contained title/group moves and eight-direction resizing now compile their
transient outer rectangle through the existing surface compiler. Contained
chrome, pane subtrees, splitters, hit manifests and semantic bounds therefore
share one layout input. Only terminal delivery mutates durable placement.
The pending-release path retains the complete projected tree until its exact
output is presented or dropped; cancellation restores the durable layout from
the retained complete measurements without requiring another host callback.
Cached measurements are reused only under their original requirement ticket.

Source-only successors retain target-layout provenance from the exact presented
scene used for solving, not from an unpresented candidate. This preserves exact
preview tokens, guide visibility and target hysteresis across a source layout
update without granting stale hover authority. Changed external measurements do
not inherit that lineage. Prepared contributions revalidate the current
projection owner and rectangle at installation, and a cancelled projected source
cannot supply new title, resize, tab or splitter source geometry before durable
geometry is presented again.

The public `with_clear_hover_lane` fact and native `ClearHoverLane` answer make
framework clearance explicit and output-bound. Core chooses the semantic winner
after each preceding input edge, including the Armed-to-Dragging transition.
Complete sources no longer occlude targets behind themselves; partial sources,
other contained roots, popups, unknown widgets and blocked releases retain their
blocking semantics. Existing `with_hover` and `with_no_hover` remain exact
assertions rather than fallbacks. The adapter paints the real projected source
once, keeps its grabbing cursor, and omits duplicate ghost/preview shells.

New and strengthened proof boundaries:

- `integration/dockspace-host-harness/tests/contained_projection.rs` covers a
  two-pane split root, continuous move, every resize direction, stable visual
  identities and projected receivers, pending Presented/Dropped settlement,
  active-output revocation, changed external bounds/minima, ordered delayed
  output settlement, and a pre-frozen same-batch behind-source docking journey
  with a blocked-release control. A complete tabs source is also presented after
  each magnetic-boundary jitter sample: its preview, active guide and final
  topology stay latched inside the release margin and change outside it.
- `integration/egui-product-harness/tests/product.rs` checks real pane callback
  rectangles, content markers, application widgets, opacity, stable widget IDs,
  one selected-item callback per pass, and matching AccessKit/chrome geometry.
  Existing complete-source gap/ghost assertions now require visible full
  content; partial-source gap coverage remains intact.
- `engine/contained_projection/authority_tests.rs` rejects the A-presented /
  B-unpresented / C-source-only target-proof splice, a prepared contribution
  installed after its owner was cancelled, and re-acquisition from a cancelled
  but still-displayed transient source. A partly offscreen source remains
  operable through its visible title, resize, tab and splitter receivers: source
  checks use the existing unclipped occlusion geometry, not the clipped frame.
- `engine/contained_projection/tests.rs` counts compilation at 16, 128 and 1,024
  panes. A batch of 64 move events compiles zero times during input and once at
  input close; repeated paint-plan queries and unchanged frames reuse that
  output. Cancellation compiles once to restore the durable layout.
- `engine/contained_projection/measurement_tests.rs` verifies that same-frame
  fresh measurement compiles only once, while measuring surface B still updates
  the active source on A. Blocked/unknown hover preserves valid source motion
  without authorizing a target or a release.
- `engine/contained_projection/selection_cost_tests.rs` counts actual pane
  visits and bar lookups during installation. The old nested lookup reproduced
  136 / 8,256 / 524,800 pane visits at 16 / 128 / 1,024 panes. The replacement
  builds one `PaneSceneId` index and stays within the linear visit budget, with
  one lookup per bar and unchanged missing-selection semantics.

Independent review and follow-up closed the clipped-source geometry defect,
fresh-measurement duplicate compilation, quadratic selection lookup, and the
cross-projection hysteresis coverage gap. The duplicate compilation and
quadratic lookup have observed failing-before/passing-after count regressions.
A narrow follow-up also reproduced and fixed source feedback loss after a
blocked/unknown-hover measurement. Its four source-acquisition paths include the
internal splitter, and the full durable workspace remains unchanged on rejection.
The native clearance concern was rejected with semantic-lane and blocker
evidence: framework clearance is not a semantic passthrough or target authority.
The original local review receipt remains a historical pre-follow-up report;
these fixes and the final test matrix are caller-owned closure evidence, not a
claim that reviewers executed Cargo or that the complete plan is finished.

Fresh functional verification on the U4 source state:

| Gate | Result |
| --- | --- |
| Root workspace, all features and all targets | 1,596 passed |
| Public host, default / serde | 27 / 27 passed |
| egui product, default / all features | 93 / 97 passed |
| Official egui harness, all features and all targets | 12 passed |
| Fork-backed native workspace, all targets | 198 passed |
| Strict rustdoc / doctests | Passed / 10 passed |
| Correctness and suspicious Clippy groups | Passed with both groups denied |
| Workspace/harness formatting and diff whitespace | Passed |

Cargo commands use one build job, the shared target directory and one nextest
test thread. Other pre-existing pedantic/style Clippy warnings are not promoted
to errors by this gate. These are deterministic library, recording-renderer and headless
adapter checks, not physical platform certification. The broader ordinary-widget
hover qualification remains U5, full commands/focus remain U6, and independent
host retained-state/latency qualification remains U12. U7/U8/U11 physical native
qualification and the remaining plan units are not completed by this gate.

## 2026-10-28 U3 journal-only gesture replacement

U3a, U3b and U3c now pass the replacement gate. `SurfaceGesturePhase`, all local
gesture-preparation APIs and `EngineInput::Local*Gesture` variants are deleted.
The local tab/splitter/contained phase reducers, contained primary-press action,
local position identity and gesture-specific `LocalReady` authority variants are
also deleted. `GestureOwner` is now a private alias for the admitted
`PointerStreamId`, not an alternate owner enum. Capture authority and drag
threshold origin are mandatory; `Authority::Unknown` remains an explicit fact,
not an absent owner. Read-only gesture views return their definite pointer and
stream identities without a legacy optional branch.

Non-pointer candidate actions remain valid without pointer/native enrollment.
Their focused modules are now `semantic_candidate.rs` and
`contained_adjustment.rs`. Accepted host-local tab activation now publishes the
same core pane-focus intent when no native viewport owns focus. Full command and
focus experience certification remains U6, rather than being inferred from this
single integration correction.

Presentation feedback tests now use public host batches and complete output
execution/presentation observations. Contained tests preserve causal press/Raise,
measurement, actual presentation and motion boundaries. The complete contained
redock Burst stays one input batch; its terminal artifact may retire when exact
preview presentation commits the topology, so it is not reused as receiver
authority. Native tests keep their actual provider domain and submit empty
checkpoints in semantic frames. The coordinator's cursor-coalescing test now
begins a real managed native press while its translator remains idle.

Two tests tied only to the deleted local-owner state are removed. Two duplicate
local splitter commit tests are covered by existing journal handle/junction
journeys; their cancellation peer is migrated to a real journal stream cancel.
The resulting root count decreases by four, without deleting presentation,
contained or native lifecycle journeys. The obsolete local-only identity
reservation is deleted; its atomic floating-counter exhaustion check now covers
the canonical composite drag reservation instead.

| Gate | Result | Scope |
| --- | --- | --- |
| Root workspace, all features and targets | 1,583 passed | Journal-only kernel, semantic candidate actions and egui adapter |
| Public engine host, default and serde | 20 passed each | Framework-neutral downstream contract |
| Product harness, default | 93 passed, none ignored | Controlled chrome, cancellation and output-bound receiver facts |
| Product harness, all features | 97 passed, none ignored | Includes the unchanged complete L03 Burst matrix |
| Official harness, all features and targets | 12 passed | Public API plus caller-owned demo compilation |
| Native workspace, all targets | 198 passed | Includes the migrated managed-domain cursor-coalescing test |
| Strict rustdoc and documentation tests | Passed; 10 doc tests | Removed APIs have no required documentation seam |
| Correctness/suspicious Clippy and formatting | Passed | Nextest uses one test thread; no concurrent Cargo commands |

Independent read-only review found no new P1/P2 in source continuation,
presentation revalidation, capture authority, threshold origin or exact-owner
cancellation after deletion. These deterministic gates do not establish full
live contained content (U4), arbitrary egui widget/blocker interoperability (U5),
the complete application loop (U6), structural cost/readiness (U12), or native
physical platform claims. The overall refactor remains active.

## 2026-10-28 U3b ordered egui transport checkpoint

The official adapter now captures the raw event packet at `run_ui` entry,
qualifies each event position against an exact accepted output, and submits the
ordered batch before measurement or paint. Run identity prevents replay during
egui discard reruns. A failed packet cannot silently retry with different facts;
the next run cancels the previous owner before accepting fresh events. Explicit
touch cancellation, focus loss and Escape terminate the same journal owner.
Changing egui Context or viewport retires the previous affine producer before
the adapter clears its routing cache.

Click and drag delivery are independent semantic lanes. In particular, contained
frame click fallbacks cannot replace title/resize drag receivers. Rear contained
content activation now uses the journal's exact click fact and the core's checked
Raise command rather than a second renderer activation frontend. Pending release
retains its frozen drop affordance as well as its preview until output settlement.

The renderer no longer reconstructs docking gesture phases from `Response`.
Keyboard, AccessKit, application commands and the existing scroll subsystem keep
their semantic paths. All remaining internal preview-only ACK inputs, factories
and reducers are deleted; their fixtures use complete output execution and exact
presentation observations instead.

Portable egui qualification is deliberately conservative. Receiver declarations
belong to one accepted output and one exact completed egui pass. The previous
pass's anonymous interactive rectangles cannot be borrowed by an older output
accepted late. Content registrations read only `this_pass.widgets`; qualification
reads only `prev_pass.widgets`, without `read_response`'s cross-pass fallback.
Both emission-time and input-time coordinates must use identity layer transforms;
an invalid coordinate domain cancels the stream and never supplies a Known raw
manipulation route. Unknown interactive widgets block this lane.
`PaneView::docking_content_widgets` lets the application
explicitly declare ordinary widgets belonging to its content slot; it is not an
automatic ancestry resolver and must not include popup/modal/foreign controls.
Each exact widget ID removes at most one anonymous rectangle, so duplicate
declarations cannot erase an unknown equal-rectangle blocker.

New product regressions cover raw close-click replay after output-hook discard,
explicit touch cancel and fresh-press recovery, late presentation versus newer
unpresented receiver facts, registered/unregistered contained content, and a
duplicate content declaration overlapping an unknown same-rectangle widget.
Context rebinding without a queued output report, transformed output followed by
an identity transform, and coordinate changes during an active resize all cancel
or reject stale authority and preserve durable placement.
Discarded-preview tests require cancellation without later implicit delivery,
then prove a fresh qualified gesture works. The resize test checks the actually
painted transient move output; it no longer requires a redundant release-frame
ghost when that preview was already accepted before release.

The original complete L03 magnetic matrix is no longer ignored. Its Burst
executor still delivers press, threshold move, target move and release in the
same raw packet; no compensating input frames were added. Ordinary rendering
and exact output settlement may happen afterward.

| Gate | Result | Scope |
| --- | --- | --- |
| Root workspace, all features and targets | 1,587 passed | Core, conformance and official adapter |
| Public engine host, default and serde | 20 passed each | Public renderer-neutral contract, no native enrollment |
| Product harness, default | 93 passed, none ignored | Includes the unchanged complete Burst matrix |
| Product harness, all features | 97 passed, none ignored | Adds persistence to the same product gates |
| Official harness, all features and targets | 12 passed | Includes caller-owned demo compilation |
| Native workspace, all targets | 198 passed | Recording/unit tests on the pinned fork; not physical certification |
| Strict rustdoc and documentation tests | Passed; 10 doc tests | Public API and compile-fail documentation |
| Correctness/suspicious Clippy and formatting | Passed | One Cargo worker and Nextest test thread |

Two independent static reviews found and closed Click/Drag lane conflation,
touch cancellation, Context/viewport retirement and transformed-coordinate
continuation boundaries. The core contained activation review found no blocker
within its scoped change. Temporarily accepting a newer pass's receiver facts
and allowing one widget ID to consume multiple anonymous rectangles made the
late-presentation and equal-rectangle blocker regressions fail respectively.
Both mutations were restored before the final passing gates.

At this earlier transport checkpoint U3 was not complete: the core
`LocalResponse` owner and local gesture frontends still required deletion and
test migration. The journal-only replacement section records their subsequent
retirement. Full contained content projection (U4), broader egui blocker
interoperability (U5), accepted semantic focus bridging (U6), public host
readiness/cost certification (U12), and physical native certification are not
established by these headless results. The following U3a/U2 sections retain their
historical gate counts and pre-transport boundaries.

## 2026-10-27 U3a host input domain and terminal convergence

The public host now owns one affine pointer producer across logical surfaces.
`enable_host_pointer` requires no native enrollment or presented surface;
`submit_host_pointer_batch` assigns one global sequence to all ordered edges.
Every edge independently qualifies manipulation/source and hover coordinates
with exact presented capabilities. Capture belongs to the host domain. Unknown
logical routes do not grant desktop outside-all authority. Existing ledger,
receiver, rollback, producer-drain and retirement mechanisms remain the owners
of transport integrity.

The downstream public consumer submits a complete press/move/release batch from
A to B, where both surfaces have identical bounds. Manipulation stays in A while
hover selects a different point in B. Actual owned recording outputs carry the
pending preview, and Presented or Dropped settlement respectively commits or
cancels the frozen release. No follow-up pointer events are synthesized. A
selection-changing press uses a separate measurement/paint cycle after the
complete input batch; pending release survives this real rendering lifecycle.

The ten new public journeys cover actual target-output settlement,
selection-changing activation, source/hover coordinate isolation, unknown
hover, dropped-frame retry and repeated release, stale route/current receiver
mixing, foreign-session capabilities, invalid manipulation after a valid press,
invalid hover during capture, and failed-suffix rollback. Additional core tests cover local
owner cancellation without panic, rear-contained Raise/resize/release,
HostLocal producer lifecycle and ledger boundaries, and locked scroll tails
whose event-time route or receiver becomes unknown.

Tab and contained release now share their actual terminal implementations
between journal and local-response frontends. Exact-owner cancellation also
uses one implementation; local session/source/target checks remain at the
frontends. A terminal splitter release with unavailable coordinates cancels
its owner instead of leaving a gesture waiting for an already-consumed release.
Legacy internal preview ACK reducers and the response-based input frontends
remain explicitly scheduled for U3b retirement.

| Gate | Result | Scope |
| --- | --- | --- |
| Root workspace, all features and targets | 1,585 passed | Core, conformance and official adapter; includes source-continuation, locked-scroll and exact hover-emission regressions |
| Public engine host, default and serde | 20 passed each | Ten new public pointer journeys; normal dependencies contain no egui, Winit or WGPU |
| Product harness, all features | 90 passed, 1 ignored | Original single-call Burst exclusion remains open |
| Official harness, all features | 12 passed | Includes caller-owned WGPU demo compilation |
| Native workspace, all targets | 198 passed | Existing egui 0.37 fork pin; physical scenarios not executed |
| Strict rustdoc and documentation tests | Passed; 10 doc tests | Workspace API documentation and compile-fail boundaries |
| Correctness/suspicious Clippy and formatting | Passed | Shared test support is compiled once; builds use one Cargo worker and Nextest thread |

Independent review and the public tests closed four input boundaries: target
hover must use its own coordinate route; logical cross-surface movement does
not require native-window drag capability; Select/Raise may retain only the
exact active source's existing continuation; and that continuation expires
when another host takes the surface's presentation stream. Hover and new
delivery remain strict current-output claims. Another pointer cannot borrow
the source continuation. Stream checks use current active ownership rather
than historical host lookup.

Two mutation checks were executed. Restoring source coordinates in the hover
comparison fails the complete cross-surface release test. Removing source
continuation fails the unselected-tab release test. Production code was restored
before the final gates. These recording and unit tests do not establish OS
input delivery or physical GPU presentation.

A follow-up proof review added exact route-authority validation before dispatching
any known HostLocal hover disposition. `NoReceiver` and `Blocked` now receive the
same exact emission binding as a `Dock` result, even though neither goes through
Dock-region geometry validation. The regression produces two actual presentations
with identical semantic output and different emissions: generic semantic receipt
validation accepts their unchanged interaction geometry, while the HostLocal
route check rejects the mixed emission. Matching route/receipt pairs remain valid.
Removing this check makes the regression fail. The workspace, both public-host
feature gates, correctness/suspicious Clippy, strict rustdoc and formatting were
rerun after restoration. Preview position remains derived from the validated hover
position; delivery and threshold continue using the independent manipulation route.

Public rollback coverage now includes a valid prefix that selects an unselected
source and begins dragging, followed by a release claiming no receiver where B's
current drop receiver wins. The batch fails after the private candidate's version
has changed. Dropping the poisoned frame preserves published selection, version,
surface proofs and interaction state. Retrying the original event order with the
correct release fact succeeds, then exact preview presentation delivers once.
This verifies producer-watermark rollback through the public retry contract,
without exposing private stream IDs or counters. Separate active-source cases
reject stale and foreign manipulation even with a valid current hover, and reject
stale and foreign hover even with valid capture and manipulation. Temporarily
reading the raw manipulation route or advancing the producer watermark from the
frame guard's Drop makes the corresponding public regression fail. Both changes
were restored before the default and serde gates passed all 20 tests each.

U3 remains open. The egui adapter still reconstructs pointer phases from its
frame-level `Response`, and its original single-call Burst matrix is still
ignored. U3b must consume ordered raw edges through the same host owner with
event-bound receiver facts, remove duplicate pointer actions/local owners and
internal ACK paths, then qualify the unchanged Burst matrix in U3c. The plan
records this split; neither the full U3 nor the full U13/AE12-AE14 matrix is
declared complete by this slice.

## 2026-10-27 U2 owned-output settlement

The official adapter now completes the same exact output contract used by engine
and native hosts. `run_ui` returns owned output plus an affine host receipt;
returning from egui does not acknowledge presentation. The actual WGPU demo owns
acquisition, submission and `queue.present`, and reports the result after present.
Receipt drop retires its exact outputs. Discarded predecessors remain owned until
the terminal receipt is disposed, so an intermediate pass cannot revoke its own
replacement while egui is still rendering it.

| Gate | Result | Scope |
| --- | --- | --- |
| Root workspace, all features and targets | 1,571 passed | Complete core, conformance and official-adapter regression suite |
| Product harness, all features | 90 passed, 1 ignored | Existing single-call Burst exclusion remains a U3 blocker |
| Official harness, all features | 12 passed | Includes WGPU demo compilation and delayed pane-focus observation |
| Native workspace, all targets | 198 passed | Existing exact egui 0.37 fork pin; smoke binaries compile, physical scenarios are not run |
| Public engine host, default and serde | 10 passed each | Downstream public API only; no egui dependency |
| Strict workspace rustdoc and doc tests | Passed; 10 doc tests | Includes compile-fail protection for both removed public preview ACK methods |
| Correctness/suspicious Clippy and formatting | Passed | Cargo builds and Nextest execution remain serialized |

Complete surface spans include ordinary geometry, panes, previews, clip data,
post-tessellation transforms and exact pixel scale. Pane callback checkpoints
reject overwriting shapes already emitted by the current program. Other-layer
snapshots retain layer identity, transform and content. Nonempty unenumerated
layers are explicitly unavailable. The marker scrubber removes only private
markers and preserves user compounds and callbacks. This is a conservative
synchronous single-layer admission contract, not inspection of application paint
callback internals or physical GPU presentation.

Independent review found and closed four boundary gaps: pane callbacks deleting
already emitted chrome, identical shapes migrating between layers, delayed focus
observations being lost before core accepts their artifact, and child receipts
waking the root viewport. Waiting focus observations remain paired with their
artifact until a committed `presents` or `retires` result; context/viewport lane
changes retire that adapter cache. A failed observation frame leaves the waiting
observations intact. Affine focus observations are consumed only after the output
has been accepted; failure of that separate submission requires a fresh pane
observation rather than duplicating its capability.

Two core corrections are also covered directly. Successful exact preview
promotion preserves its still-current target and source gap, while Dropped
successors still revoke it. Identical complete measurement and coordinate facts
retain the immutable Ready candidate; changed facts still replace it, and every
concrete paint keeps a distinct emission key. Replacement fixtures now change
geometry explicitly instead of relying on identical remeasurement to force a
new scene stamp.

Three mutation checks were executed: removing the positive-promotion guard,
removing the pane append check, and dropping waiting focus observations each
made its corresponding behavioral regression fail. Restoring the production
code passes the final gates above. The redundant renderer-pass bridge and both
public preview-only ACK methods are deleted. U3's manipulation-owner and ordered
logical-surface input work remains separate; no native multiview or real-window
interaction completion is inferred from these headless gates.

## 2026-10-27 U14 and dependency migration baseline

The implementation preserves the audit below as historical failure evidence.
The following results were freshly executed after the U14 fixes, public host
contract work, and official egui 0.37.0 migration. Cargo builds were serialized
with `CARGO_BUILD_JOBS=1`, and Nextest used `--test-threads=1`.

| Gate | Result | Scope |
| --- | --- | --- |
| Workspace, all features and targets | 1,566 passed | Official egui 0.37.0; includes three new failed-output/focus regressions |
| Independent public host, default features | 10 passed | Owned geometry/content/identity records, exact present/drop and commit authority, out-of-order results, two logical surfaces and layout commands |
| Independent public host, `serde` | 10 passed | Includes dual-stream receiver isolation, retired and stale output rejection, automatic output drop, popup barrier retention, and failed core commit/retry |
| Product harness, all features | 90 passed, 1 ignored | Official egui/kittest 0.37.0; existing Burst exclusion retained |
| Official consumer, all features | 9 passed | Includes compilation of the caller-owned WGPU demo |
| Native workspace, egui 0.37 fork | 180 passed | Remote exact pin `0716beee5b7284389f2e80ffb6b24b5af6ae2910`, three test binaries |
| Complete native workspace, including smoke unit targets | 198 passed | Five test binaries; this compiles the smoke executables but does not run a real-window scenario |
| Native library, debug assertions disabled | 173 passed | Same remote 0.37 pin |
| Fork egui and egui-wgpu libraries | 112 passed | Includes five new real elastic-scroll consumption/snapshot regressions and retained-overlay test |
| Fork eframe library, Glow + WGPU + native seam | 53 passed | Host finalizer, output identity, create, retention and renderer seams |

The native fork branch `dockspace-native-seam-0.37` is committed and pushed.
Manifests, lockfile, CI checkout, README, and seam admission agree on the exact
revision. Winit remains at `f378b50f29cafafec31cd8675c17646b4ef57d70`.
The 0.37 scroll query preserves source-dependent elastic consumption as
`Unknown` rather than publishing known-empty. Offscreen screenshots preserve
their callback path without falsely reporting surface presentation.

The public core acknowledgement now cross-checks the actual committed scene's
accepted exact emission instead of echoing a host `Presented` result. Retired
surfaces and invalidated scenes settle without promotion. Valid proofs waiting
behind a multi-surface popup barrier remain retainable until the sibling surface
presents; their receiver bindings stay unavailable until the barrier completes.

Workspace correctness/suspicious Clippy gates, strict core/adapter rustdoc,
eight documentation compile-fail tests, and formatting across the root and
independent workspaces pass. Real-window and physical X11 scenarios were not
rerun on this macOS host; platform claims require their separate CI evidence.

U14 removes required mutations from six `debug_assert!` expressions and promotes
native receivers immediately after successful core commit. The old shared
acknowledgement queue is deleted; idle hosts no longer depend on follow-up input
to consume a committed presentation. A regression test failed before this fix
and passes afterward:
`receiver::tests::committed_host_frame_promotes_receivers_without_follow_up_input`.
Before upgrading the native fork, the repaired 0.36 baseline passed all 180
native workspace tests and all 173 native library tests with debug assertions
disabled. CI now carries the assertion-disabled gate.

The final review also required an exact-emission regression inside
`receiver::tests::exact_hit_without_hover_lane_is_unclaimed_while_uncertain_hits_fail_closed`.
The test now emits the same semantic scene twice in one session with identical
binding and coordinates, presents the newer output, and verifies that its native
query accepts only the newer artifact. Temporarily removing the emission-key
comparison made that test fail at the old-artifact rejection; restoring the
comparison passes. This distinguishes exact output identity from semantic scene
equality.

One non-blocking coverage recommendation remains: exercise managed-host renderer
termination through `prepare_output_prefix`, abandon the subsequent host frame,
then retry and verify exact promotion and one boundary release. Current core
failed-commit/retry, idle ACK, and atomic receiver tests cover its component
contracts, but are not that complete coordinator path. No production defect was
confirmed in this path by the review.

The public recording consumer does not register OS windows or a pointer
provider. Its same-coordinate surface tests use prepared tab actions; they
do not prove ordered multi-surface pointer routing or the complete 60-case
gesture matrix. The actual public UI event pipeline remains U3 work. Complete
program acceptance is explicitly documented as separate from GPU submission
and final presentation; core accepts the exact reported prefix in a later
successful host-frame commit.

### Remaining U3 replacement boundary

The official adapter now preserves complete execution receipts and uses the same
exact `PaintedSurfaceOutput` present/drop path as public engine and native hosts.
Both public preview-only ACK methods and the redundant renderer-pass bridge are
deleted. Caller-owned egui runs return `DockspaceOutput`; the actual renderer
owner takes the complete output and reports its terminal result exactly once.
No-report, receipt drop, output mismatch, discarded predecessor, foreign lane,
unwind, delayed report and pane-focus eligibility are covered by real emissions.

The official integrity check brackets the complete surface span and compares all
ordered shapes, clip rectangles, post-tessellation transforms and pixel scale
after output hooks. Its explicit admission limit is synchronous, append-only pane
painting on the supplied layer. Cross-layer writes are unavailable rather than
silently omitted. Application callbacks must honor source-gap opacity and clip;
the adapter does not inspect their renderer internals. Once output ownership is
split, the host must preserve the complete output it reports. Shape membership
never proves GPU submission or physical presentation.

U3 still replaces the independent local-response and journal terminal reducers,
supports ordered logical-surface routing, and removes the known-invalid Burst
exclusion. U2 output integration does not claim those later gates.

## 2026-10-27 audit baseline (before repairs)

The audit checked `3216855` plus the existing 26 uncommitted U2b files. These
results describe that worktree, not a clean release or the planned egui upgrade.
All Cargo builds used `CARGO_BUILD_JOBS=1`; Nextest used `--test-threads=1`.

| Gate | Fresh result | Limit |
| --- | --- | --- |
| Workspace, all features and targets | 1,563 passed | Does not include the excluded native workspace |
| Product harness, all features | 90 passed, 1 ignored | Full Burst matrix remains excluded |
| Ignored L03 complete matrix, explicitly selected | Failed | First Center/Burst sample `(280, 280)` has no magnetic preview after eight causal observations |
| Official consumer harness | 9 passed | No real-window run was performed in this audit |
| Native workspace, default test profile | 179 passed | Debug assertions enabled |
| Native library, debug assertions disabled | 164 passed, 8 failed | 172 library tests; does not include the other native test binaries |
| Workspace and native formatting checks | Failed | Existing U2b formatting differences remain |
| `git diff --check` | Passed | Patch whitespace only |

The native assertion-disabled run used:

```text
CARGO_BUILD_JOBS=1 cargo nextest run --manifest-path integration/egui-fork-workspace/Cargo.toml --package egui_dockspace_native --lib --locked --test-threads=1 --no-fail-fast --config 'profile.test.package.egui_dockspace_native.debug-assertions=false'
```

Required mutations in `NativeCoordinator::prepare_output_prefix`,
`HostRecords::reserve_output`, and `HostRecords::settle_viewport_roster` occur
inside `debug_assert!` arguments and disappear when assertions are disabled.
The failures cover six ingress/create/input lifecycle cases and two terminal
roster settlement/rearm cases. U14 owns this correction and its configuration gate.

The current U2b receiver path already waits for a successful core presentation
commit before promoting an artifact. That is stronger than the older immediate
physical-callback promotion. Remaining work includes idle ACK draining,
consistent commit exits, and removal of local preview-only ACK authority;
passing default tests do not establish those deletion gates.

No physical OS-input, multi-monitor, GPU submission, or current-release package
validation was rerun in this audit. The selected egui 0.37.0 migration and the
new public engine-host harness remain implementation work under U15 and U13.

## Historical U1 journey directory (superseded by dated qualifications above)

| Journey | Semantic source / basin | Preview | Outcome / ownership | Focus / quiescence | Cadences | Executors and current status |
| --- | --- | --- | --- | --- | --- | --- |
| L01 | Tab body; below-threshold select or threshold-crossing drag | None before selection | Select without drag; source remains owner | Selected tab; local quiescence | Burst, Stepwise | `core-host` and `egui-product-headless` characterize selection; exact human threshold remains product evidence |
| L02 | Tab body; semantic tab gap before/after a sibling | Visible insertion candidate | Reorder once; local target owns the tab | Moved tab; local quiescence | Burst, Stepwise, ReleaseWithFinalMove, DiscardBeforeRelease | Both existing shared product/core paths cover the local command spine; product drag evidence is normative, pass-count details are not |
| L03 | Tab body; pane center plus north/south/east/west basins | Magnetic candidate, independent of guide targeting | Center merge or edge split; target owns payload | Moved tab; local quiescence | Burst, Stepwise, Hesitant, ReleaseWithFinalMove | `core-host` passes all 60 finite cases; `egui-product-headless` passes the 45 Stepwise/Hesitant/ReleaseWithFinalMove cases. A crate-private renderer receipt proves each product point remains outside every registered guide receiver. A single-call product Burst remains an explicit blocker because no Dock preview is published before the causal budget expires. |
| L04 | Contained title/edge; surface free space and eligible pane basin | Full contained-window projection | Move/resize/re-dock commits once; contained or target owner | Moved item; local quiescence | Stepwise, Hesitant, ReleaseWithFinalMove | Durable placement and ownership are covered; full-window projection remains an intentional known-invalid baseline |
| L05 | Tab body; current basin can become stale or disappear | Candidate is cleared while source stays recoverable | Cancel/reject and restore source without partial mutation | Preserve source focus; local quiescence | Stepwise, TargetDisappears | `core-host` stale-target and target-destruction fixtures provide the current normative evidence |
| L06 | Tab body; center/edge basin with release barrier | Exact candidate must be presented before release commits | Commit presented candidate or restore source | Moved item or preserved source; local quiescence | Burst, Hesitant, ReleaseWithFinalMove, DiscardBeforeRelease | Discarded-preview rejection is normative; release-wait and ordinary fixed-hover tests remain characterization or known-invalid until causal readiness replaces fixed frames |
| L07 | Group handle, contained title, menu, and tab close; no overlapping owner | Group/command preview only where applicable | One semantic owner executes each chrome action | Close policy chooses successor; local quiescence | Burst, Stepwise | Close-region and group-drag tests characterize the current ownership boundary |
| L08 | Splitter, tab, close, keyboard, AccessKit, and touch sources; semantic command target | Splitter proposal where applicable | Shared prepared command commits layout action | No focus guessed from dwell; local quiescence | Stepwise, TouchLongPress | Existing shared local scenario runs through `core-host` and `egui-product-headless`; touch long-press has a typed capability gap |
| N01 | Contained/native title; outside-all release | Native pending while source remains recoverable | Create → first-live → redock → retirement | No inferred pane focus; native lifecycle quiescence | Stepwise | Core-host managed lifecycle passes; official/product hosts remain capability-negative without native facts |
| P01 | Tab body in one window; center/edge basin in another window | Completed-pass hover candidate | Physical redock under one qualified handoff | Moved item; native lifecycle quiescence | Burst, Hesitant, ReleaseWithFinalMove | X11 physical smoke is characterization evidence; other hosts fail closed until their physical gates pass |

The current shared local journeys are the finite L03
`MagneticDockingBasins` matrix and the existing L08
`LocalTabCloseSplitterAndContained` scenario. L03 expands one source, five
basins, three normalized non-guide samples per basin, and four cadences without
becoming a general GUI language. Both executors emit normalized
`ItemDocked`/`Quiescent` observations; product readiness uses a causal predicate
with a diagnostic upper bound.

- `crates/dockspace_host_conformance/src/tests.rs::shared_l03_magnetic_docking_scenario_maps_to_core_host_semantics`
- `crates/egui_dockspace/src/product_dockspace_tests.rs::shared_l03_supported_points_use_renderer_registered_magnetic_receivers`
- `integration/egui-product-harness/tests/product.rs::shared_l03_supported_magnetic_docking_maps_to_egui_product_semantics`
- `integration/egui-product-harness/tests/product.rs::shared_l03_complete_magnetic_docking_matrix_maps_to_egui_product_semantics` (ignored known-invalid Burst gate)
- `crates/dockspace_host_conformance/src/tests.rs::shared_local_interaction_scenario_maps_to_core_host_semantics`
- `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics`

N01 uses the existing lifecycle scenario and remains a separate capability
boundary. P01 is never inferred from a local or official-egui run.

## Historical U2a ordered SurfaceProgram evidence

The ordered traversal slice is verified independently from the later atomic
presented-artifact and host-terminal-settlement work. Core, product-headless,
and fork-backed native tests now consume `SurfacePaintPlan::visit_surface_program`
for root operations, popups, previews, guides, source-gap disposition, and
feedback. Surface-wide flat plan iterators are crate-private or test-only, and
the deleted synthetic walker cannot recreate a missing operation from parallel
records. This evidence proves traversal membership and renderer order; it does
not claim `RendererPassBridge` promotion, final host settlement, or unwind
abandonment.

Current gates:

- `cargo nextest run -p dockspace --lib --locked -j1 --no-fail-fast`:
  1430/1430 passed on 2026-08-31.
- `cargo nextest run --manifest-path crates/dockspace_host_conformance/Cargo.toml --locked -j1`:
  37/37 passed on 2026-08-31.
- `cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --locked -j1`:
  86/86 passed with one explicitly skipped test on 2026-08-31.
- `cargo nextest run --manifest-path crates/egui_dockspace_native/Cargo.toml --locked -j1`:
  178/178 passed on 2026-08-31.
- `cargo check -p egui_dockspace --all-targets --locked -j1`: passed.
- `cargo check --manifest-path integration/egui-fork-workspace/Cargo.toml --workspace --all-targets --locked -j1`:
  passed.

The remaining atomic artifact, durable discarded-pass action queue, official
host finalizer, and native unwind/retirement paths stay deferred to the next
settlement slice.

## Historical U1 oracle matrix

Each row is classified as `normative`, `characterization-only`, or
`known-invalid`. Only normative rows may gate U1 completion. A source is listed
only when the named test or function exists in the repository; an intentional
gap is written as such instead of being given a synthetic test name.

| Row | Journey / concern | Classification | Existing evidence or explicit gap | Decision |
| --- | --- | --- | --- | --- |
| U1-01 | L01 action: click selects a tab | normative | `integration/egui-product-harness/tests/product.rs::default_features_click_selects_a_tab` | Preserve selection without drag |
| U1-02 | L01 retained frame: drag decoration after threshold | characterization-only | `integration/egui-product-harness/tests/product.rs::active_tab_drag_leaves_an_accessible_gap_and_paints_one_pointer_ghost` | Keep as visual baseline; fixed deltas do not gate behavior |
| U1-03 | L02 action: reorder commits once | normative | `integration/egui-product-harness/tests/product.rs::default_features_tab_drag_reorders_across_multipass_discard` | Preserve item order and ownership |
| U1-04 | L03 presentation: outer guide targeting | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_paint_outer_guides_and_dock_each_edge` | Visual guide coverage only; exact guide targeting is not docking acceptance evidence |
| U1-05 | L03 presentation: center guide hit | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_center_guide_merges_tabs` | Do not promote guide aiming to the product contract |
| U1-06 | L04 action: durable contained move/resize/redock | normative | `integration/egui-product-harness/tests/product.rs::contained_move_resize_select_focus_and_dock_back_preserve_exact_ownership` | Durable placement changes only at terminal commit |
| U1-07 | L04 presentation: full-window contained drag preview shell | characterization-only | `integration/egui-product-harness/tests/product.rs::contained_title_drag_paints_a_full_window_preview_in_the_same_run` | The window-sized shell is present; live pane-content projection remains a later U4 gate |
| U1-08 | L05 action: stale target has no partial mutation | normative | `crates/dockspace_host_conformance/src/tests.rs::private_multi_surface_fixture_rejects_a_stale_target_without_partial_commit` | Keep fail-closed stale rejection |
| U1-09 | L05 presentation: target loss preserves source drag | normative | `crates/dockspace_host_conformance/src/tests.rs::private_multi_surface_fixture_target_destruction_preserves_the_source_drag` | Clear only the lost target route |
| U1-10 | L06 retained frame: discarded preview cannot release | normative | `integration/egui-product-harness/tests/product.rs::default_features_discarded_preview_cannot_release_a_drag` | Release needs current presented evidence |
| U1-11 | L06 presentation: release waits for exact preview | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_release_waits_for_the_new_preview_to_be_painted` | Retain the regression baseline, but its exact coordinates and fixed preparation/settlement frames are not causal preview-readiness proof |
| U1-12 | L06 retained frame: ordinary readiness via fixed hover loop | known-invalid | `integration/egui-product-harness/tests/product.rs::default_features_paint_outer_guides_and_dock_each_edge` | Replace fixed ordinary waits with causal predicates |
| U1-13 | L07 action: close receiver stays disjoint from tab receiver | normative | `integration/egui-product-harness/tests/product.rs::tab_close_semantics_follow_the_exact_close_region_occlusion` | One close owner, no action stealing |
| U1-14 | L08 action: shared local observations are executor-parallel | normative | `crates/dockspace_host_conformance/src/tests.rs::shared_local_interaction_scenario_maps_to_core_host_semantics`; `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics` | Keep the shared scenario narrow and normalized |
| U1-15 | L08 action: touch long-press | known-invalid | No existing executor test; `ScenarioCapability::TouchLongPress` remains unsupported | Preserve typed capability-negative evidence |
| U1-16 | N01 first-live ordering | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Record current causal ordering; do not infer an ownership verdict here |
| U1-17 | N01 retirement | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Preserve as lifecycle evidence until runtime replacement requalifies it |
| U1-18 | N01 quiescence | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Measure bounded cleanup, not callback count |
| U1-19 | P01 physical drag barriers | characterization-only | `integration/egui-native-smoke/src/bin/physical_x11/driver/scenario.rs::perform_drag_out`; `::move_to_center_guide_while_pressed`; `::native_drag_preview_barrier` | X11 is evidence to requalify, not a portable authority |
| U1-20 | P01 missing native facts | known-invalid | `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics` (admission assertion only) | No executor performs the physical gesture, reports the exact missing native facts, and proves source ownership unchanged; keep the gap explicit |
| U1-21 | L03 ordinary magnetic basin matrix | normative | `crates/dockspace_host_conformance/src/tests.rs::shared_l03_magnetic_docking_scenario_maps_to_core_host_semantics`; `crates/egui_dockspace/src/product_dockspace_tests.rs::shared_l03_supported_points_use_renderer_registered_magnetic_receivers`; `integration/egui-product-harness/tests/product.rs::shared_l03_supported_magnetic_docking_maps_to_egui_product_semantics` | Core-host proves 5 basins × 3 normalized non-guide samples × 4 cadences; the crate-private product receipt proves the 45 supported product cases remain outside every renderer-registered guide receiver; the black-box product executor proves preview, true same-call ReleaseWithFinalMove, normalized docking, and stable quiescence |
| U1-22 | L03 single-call product Burst | known-invalid | `integration/egui-product-harness/tests/product.rs::shared_l03_complete_magnetic_docking_matrix_maps_to_egui_product_semantics` (ignored; run explicitly) | The product executor reaches the first Burst case without a magnetic Dock preview after eight causal observations; retain the failing gate for the later manipulation-kernel replacement rather than substituting guide or fixed-frame success |

The N01 rows deliberately do **not** classify “native ownership before
first-live” as known-invalid. The existing fixture demonstrates a sequence,
but this U1 baseline does not promote that observation to an ownership verdict.
That comparison belongs to the later lifecycle replacement unit.

## Historical U1 capability boundaries

| Executor | L01–L08 | N01 | P01 |
| --- | --- | --- | --- |
| `core-host` | L03 full 60-case matrix and L08 shared command scenario pass | Programmatic lifecycle scenario passes | Unsupported without `NativePhysicalAuthority` |
| `egui-product-headless` | L03 Stepwise/Hesitant/ReleaseWithFinalMove 45-case matrix and L08 shared command scenario pass; L03 Burst is known-invalid | Capability-negative | Capability-negative |
| `egui-official-harness` | Public local facade; the caller-owned demo supports exact terminal-output settlement, while unmodified eframe examples remain capability-negative | Capability-negative | Capability-negative |
| `native-coordinator-trace` / `egui-native-smoke` | Adapter-specific evidence | Programmatic lifecycle evidence | Physical X11 evidence only |

Capability-negative is a successful result when the exact missing fact and
unchanged source ownership are reported. No executor may substitute a rectangle
scan, final hover, fixed guide center, fixed ordinary frame count, receipt, or
internal scene/graph identity for a missing fact.

## Historical U1 verification notes

- Shared scenarios contain finite semantic sources, basins, normalized inset
  samples, cadences, selectors, and expected observations only. Coordinates,
  graph/scene identifiers, receipts, and frame counts stay in private executor
  code or core geometry tests.
- Ordinary product readiness must use a causal predicate with a bounded
  diagnostic timeout. Fixed pass counts remain valid only when discard/terminal
  pass count is itself the contract.
- The remaining contained-content projection, overshoot/release-hysteresis,
  product Burst, and touch capability gaps are retained as explicit evidence. The window-sized
  preview shell is now covered separately and is not treated as full content
  projection until U4.
- The lightweight native example is a discoverability checkpoint. The complete
  game-editor showcase and layout/preset shell remain assigned to U10.
