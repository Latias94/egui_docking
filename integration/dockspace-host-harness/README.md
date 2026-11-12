# Public host contract harness

This independent workspace uses `dockspace`'s public API as its only production dependency.
Its `serde` feature forwards to `dockspace/serde`; it has no UI or platform dependencies.

`tests/shared_local_scenario.rs` consumes the same L08 semantic scenario as the
core conformance and egui product executors. It resolves prepared actions from
the current public paint plan, checks real selection/close/resize results and
settles exact recorded outputs. The conformance crate is a test-only dependency;
production integration still depends only on `dockspace`. See the
[adapter testing guide](../../docs/ADAPTER_TESTING.md) for the boundary between
these recording checks and qualification of a concrete UI framework.

`RecordingRenderer` accepts the complete ordered surface program, copies geometry,
opaque identity, receiver descriptors, and pane-content requests into owned records,
and returns the core execution receipt. Recording is sink acceptance, not final presentation.
The tests inspect records after frame borrows end and report exact affine outputs as
`Presented` or `Dropped`. Only a committed core acknowledgement grants receiver authority.
They also cover independently settling pending surfaces with identical receiver coordinates,
popup owners presenting independently of deferred siblings, late outputs
after whole-surface redocking or pane-selection invalidation, abandoned outputs unblocking
newer presentation prefixes, and semantic layout commands without registering a pointer
producer or enabling native hosting.

`update_publication`, `presented_semantic_scope`,
`presentation_admission_scope` and `deferred_surface_actions`. They exercise
same-update measurement/encoding, owned suspension, repeated encoding, bounded
admission, exact immutable in-flight scenes, stale source/target rejection and
scoped actions from discarded passes. The opt-in `frame_cost` example measures
complete updates separately from output settlement; its methodology is recorded
in [BENCHMARKING.md](BENCHMARKING.md).

The host input tests submit complete ordered batches across two logical
surfaces with identical bounds. Source manipulation and target hover carry
independent exact presentation routes. Ordinary release commits once with its
accepted input, and later dropped output does not undo it. Negative journeys exercise unknown
routes, stale or foreign authority during an active gesture, failed frames, and
producer retirement. A wrong-winner release after a selection-changing valid
prefix must roll back the published version, selection, interaction and surface
proofs; retrying the same ordered edges must deliver exactly once. The retry
contract also detects producer watermarks advancing during rollback.

The contained-window tests check full content during move and every resize
direction, immediate ordinary release and exact output settlement, cancellation, changed measurements,
and a target behind the complete source in a pre-frozen input batch. A complete
tabs-root journey presents each source-only projection while jittering across a
magnetic boundary: the target stays latched inside its release margin and changes
outside it. Framework-clearance facts never replace the core target solver.

`tests/nested_splitter.rs` compares nested touching-boundary resizing through
pointer and semantic actions. It checks distant-pane pixel preservation,
descendant minimum limits, central leaves, N-ary splits and four-arm junctions
with immediate or deferred output handling. Failed and abandoned frames publish
none of the proposed split updates; a successful junction release applies the
whole set once. Returning to the original boundary preserves weight bits and
does not change the workspace version.

`tests/docking_enabled.rs` submits ordered targeting directives both on real
edges and as modifier-only semantic controls. Its recording outputs cover
suppression, exact rollback, foreign-pointer isolation, stale-preview rejection,
fresh reacquisition, explicit targets and an already frozen release. Modifier
controls query no receiver and advance no pointer sequence. Core regression
tests additionally preserve complete contained geometry and allow an exact
known keyboard-focus Root to control its provider's active Child gesture.

This is public-contract and renderer-neutral input evidence. It does not prove
physical OS event delivery or actual GPU presentation. The egui product and
official harnesses separately exercise Burst input and ordinary pane content.

`tests/direct_prepared_native.rs` supplies window and renderer readiness through
public `Created` and `Prepared` facts. Its recording sink verifies complete
prospective child content, exact first-live publication, cancelled/failed/late
output, readiness revocation, foreign create acknowledgements, source-version
revalidation and replacement incarnations. A replacement's readiness recovery
refreshes its semantic output without redundant measurement. Creation can be acknowledged before visibility
is known while preserving preparation and semantic first-live barriers. These
are host-contract traces; actual platform creation and presentation remain host
responsibilities. The native adapter owns any hidden/show renderer steps and
has separately labelled native reference qualification.

`tests/native_surface_creation.rs` creates a native window for a complete
logical surface that already owns its main root and contained forest. Reported
preparation and exact semantic first-live presentation preserve topology and
workspace version. Failed and cancelled initial attempts retry through the same
recovery owner with a fresh binding; old Create acknowledgements, outputs,
Destroyed observations and producer quiescence cannot
admit or retire the successor. Finite producer storage, same-frame cancellation
and dropped/failed-frame replay exercise the public boundary. These recording
traces do not qualify automatic native document recreation or OS creation.

`tests/native_document_restore.rs` exercises the opt-in atomic native restore
boundary using only public APIs. It preserves complete split/selected-tab/
contained forests with their declared identities, queues explicit Child intents,
waits for fresh Root facts and all inherited exact predecessor guards, and
advances creation under finite producer/effect-report storage. Created or unknown
preparation retains the complete forest until a Prepared binding presents its
exact first-live output. Stopped intents stay suspended until explicit retry;
invalid declarations and retained Root/Child conflicts
publish nothing. Recording results remain distinct from the native example's
separate real-window restoration journey.

`tests/multi_root_restore.rs` restores explicitly mapped external Roots in one
transaction, including a complete surface swap. Proposed routes publish only
after commit; failed or dropped attempts preserve all predecessors and document
state. Restored Children retain their distinct recovery hosts through renderer
preparation, first-live presentation and resource loss. Duplicate, stale,
foreign and occupied mappings reject without discarding earlier accepted input.
The same batch continuation contract applies to layout reset without serde.

`tests/programmatic_native_tear_off.rs` promotes one item, an opaque captured
tabs group or a complete root directly into the existing native create owner.
It checks source preservation until exact first live, group/version authority,
failure and cancellation, explicit/default placement, policy and finite storage.
Central group transfers leave the original central container empty. These
recording traces do not qualify new physical multiview or mixed-DPI profiles.

The explicit Root-rebinding journeys remove the old logical host while preserving
its external window token under a new exact binding. They force a retired incoming
Root through exact instance replacement, check Child recovery/placement references, failed/dropped
frame retries and old-output isolation, and keep the existing first-live gates.
`native_local_geometry.rs` also exercises layout-only Root continuation without
enabling persistence. These recording tests prove host semantics rather than an
OS window's survival; the optional native reference supplies that separate proof.

`tests/native_local_geometry.rs` measures and presents a native root with client
size and independent scales, without a desktop origin or work-area facts. It
checks stable selection, exact output, frame retry and atomic rejection of
conflicting sizes. A desktop-input negative case proves that local geometry
cannot supply a missing desktop origin. These are recording-renderer geometry
and command traces; they do not qualify native local pointer gestures.

`tests/native_local_pointer.rs` uses the joined native recorder and opaque
committed local-coordinate capabilities for full same-window tab reordering.
It covers exact deferred presentation, dropped frames/outputs, geometry and
binding isolation, stale samples behind newer snapshots, outside/Unknown
terminals, inactive-tab source continuation and choosing a new lane for the next
gesture on the same pointer. These public host traces use no desktop origin or
work area for local operations; they do not qualify Winit/eframe event capture,
positive in-gesture handoff or native stationary scheduling.

`tests/native_pointer_handoff.rs` adds explicit same-event local/desktop packets
to the joined journal. Its public journeys retain one gesture through an atomic
coordinate-lane upgrade, including inactive-tab continuation, frame rollback,
Dropped and late outputs, exact source/hover mappings and a confirmed cross-window
drop. Invalid continuity or mapping preserves local terminal handling; physical
routing and target-output presentation remain independent requirements. The
public contract does not prove OS callback emission or native stationary
scheduling.

`tests/native_stationary.rs` and `tests/native_stationary_targets.rs` exercise
the Native-specific continuation projection, hover dwell, Touch long press,
tap/yield, exact-output edge scrolling and expanded Desktop Touch resize.
They use fresh fact packets at the requested deadline, rather than replaying
cached capture or position. Negative journeys cover obsolete tokens after
handoff, wrong pointer/reused stream, capture loss through button/scroll facts,
stale receivers, coordinate rebase and whole-frame retry. The handoff fixture
also verifies cross-window dwell behind confirmed Applied routing. This public
contract does not certify adapter scheduling or OS stationary/Touch observation.

The native stationary fixture also reads the desktop mouse drag's capture
requirement before timed work exists, checks stable identity across hover
changes and dropped frames, and distinguishes the next gesture on a reused
pointer. Pending input hides the projection until commit; local and Touch
gestures, foreign sources, cancellation and retirement cannot borrow it. These
queries neither acquire native capture nor prove that native cleanup finished.

The public close consumer completes item/root close plans with Allow, Veto and
exact deferred continuations, independently of pointer registration. Feedback
tests consume pending and terminal statuses by stable manipulation identity,
check publication deduplication, and preserve active-gesture cancellation on producer
retirement. These recording tests distinguish command/feedback semantics from
adapter focus execution and physical presentation.

`tests/layout_reset.rs` submits application defaults on an ordinary final host
frame or after a budgeted native prefix. A real presented receiver is answered
before an atomic input group yields; both final `poll + submit` and synchronous
convenience paths process only its remaining edge. Dropped attempts replay from
the published watermark, while a committed reset publishes one replacement and
the complete native root successor roster. Reset preserves declared semantic
identities, rejects stale/foreign/locked preparations and retires old output
and focus authority.

With serde enabled, `tests/terminal_restore.rs` adds new recognized document
keys only at the terminal restore, after earlier input used the old identity
scope. It verifies failed/dropped publication, append-only key history and save
generation, document-sidecar invalidation, native root replacement and unloaded
application content. These are core host-contract models, not automatic native
window recreation or the native showcase's preset UI.

`tests/surface_presentation_preferences.rs` records explicit application-root and
owned-child hosting through public native registration, including a child's main
root and complete contained forest. The document round-trips those declarations
with optional physical placement. Prepared, candidate-frame and committed
projections preserve the same semantic child and recovery-host references. Dropped attempts preserve the published document, and reads
cannot create effects, native bindings or presentation authority. The single
schema version is 1, with ordinary nested component objects. Unsupported versions
reject before payload decoding or application-key recognition.

`examples/engine_host.rs` is a runnable integration walkthrough using this same
recording sink: two logical surfaces, deferred owned output, Presented/Dropped
settlement, application focus, stable-item commands, per-item root-close approval
and feedback deduplication. The host keeps its rendering and focus state.

`tests/owned_output_cost.rs` counts measurement requests, public program steps,
owned records and retained recording-buffer bytes at 16, 128 and 1,024 panes.
Its structural bounds complement the core compiler/resolver counters; they do
not assert a wall-clock budget, zero allocations or bounded GPU resources.

`tests/host_coordinate_domains.rs` runs the same complete cross-surface batch
with immediate and deferred host consumption, two logical origins, two window
DPI values and two independent pairs of framebuffer densities. Only the host
interprets the synthetic pixel input and emits framebuffer commands; core sees
independent logical manipulation and hover routes. This is mapping-contract
evidence without real OS input or GPU presentation.

Run the independent gates from the repository root:

```text
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --locked --test-threads=1
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --features serde --locked --test-threads=1
cargo run --manifest-path integration/dockspace-host-harness/Cargo.toml --example engine_host --locked
python3 scripts/check_core_dependencies.py
```

`tests/logical_surfaces.rs` exercises incremental empty-central attach and retire
without native enrollment or workspace replacement. It preserves an unaffected
surface's exact presented receiver, docks content into the new central region,
migrates it back, retires the empty surface, and reuses its semantic IDs without
reactivating predecessor outputs. Typed policy/nonempty/native/recovery refusals
and abandoned/poisoned-frame retries preserve published state.
The successor can present before predecessor outputs settle; late predecessor
results and exact stream reclamation cannot disturb the successor's authority.

`tests/popup_scroll_continuity.rs` keeps a sibling's presented output through a
popup open/close cycle. A new smooth-scroll sequence uses current semantic popup
routing without remeasuring that sibling. An existing sequence still terminates
when the popup routing actually changes.

`tests/deep_layout.rs` compiles, clones, drops and measures a 10,000-level product
declaration on the ordinary thread stack. Error cleanup is included. The recording
program and exact final presentation cover the production scene path rather than
only the low-level graph solver.

`tests/native_work_area_placement.rs` checks default window placement from current
authoritative work areas: greatest rectangle intersection, nearest-rectangle
fallback and deterministic token ties, followed by the existing cascade/clamp.
Unknown/foreign/empty facts cannot reuse a historical placement authority.

`cargo run --release --example frame_cost` prints five local samples each for idle
and selected-item host frames at 16/128/1024 panes. It has no time threshold and
is not platform-latency certification. Core work counters separately distinguish
shared immutable workspace snapshots from explicit command-stage graph copies.
