# Dockspace Public API Seal

## Status

This document describes the target breaking boundary for the first shipping
`dockspace` API and records the portions already sealed. `egui_dockspace` no
longer re-exports the core or exposes its raw `DockEngine`; application-facing
types remain at the crate root, while renderers and native hosts use the typed
`DockspaceSession` runtime facade. The renderer-neutral crate keeps reducer,
frame, scene, pointer, effect, recovery, viewport FSMs, raw graph, checked
command, transaction, canonicalization, validation, and runtime identity modules
private under every feature combination. Product construction uses
`DockspaceLayout`, read access uses `DockspaceView`, and mutations use
revision-bound product actions. Persistence and close paths still need further
consolidation into the target facade areas, so the seal remains an active
breaking refactor.

## Goal

Expose a small renderer-neutral contract in which `dockspace` is the sole
authority for topology, transactions, presentation planning, interaction, and
surface lifecycle. UI adapters provide measured facts, paint a core-derived
plan, report explicit presentation observations, and execute platform effects. They
must not construct scenes, infer route authority, or mutate docking state.

The stable boundary should support an egui adapter, an Open GPUI adapter, and a
future native viewport runtime without making a renderer's frame scheduler,
widget tree, or motion implementation part of the core ABI.

## Non-Goals

- Preserve pre-seal names through compatibility aliases.
- Make every current diagnostic, reducer transition, or test fixture stable.
- Share renderer execution, animation clocks, accessibility trees, or platform
  handles between adapters.
- Treat the direct-engine protocol harness as proof of adapter conformance.

## P0: Presentation Observation Capability

A scene stamp identifies compiled geometry; it does not prove that an adapter
presented that geometry. The public observation contract must therefore expose
an opaque `SurfacePresentationOutputTicket` after a successful ready publication,
core-minted host, stream, and emission identities after actual paint, and an
opaque `PresentedSurfaceAuthority` only after a later exact observation.

Neither capability may publicly expose `SurfaceSceneStamp`, internal emission
serials, coordinate generations, or authority-domain identities. Public
accessors are limited to facts an adapter needs to route a later operation, such
as the logical surface and, when applicable, the exact native binding. The core
must validate stream identity, retirement watermarks, stale fallback, exact
binding incarnation, and coordinate changes.

This is a prerequisite for the pointer-edge journal: journal entries carry a
presented authority capability, never a naked scene stamp.

## Stable Facade Sketch

The exact names may change during implementation, but the stable API is limited
to these roles:

| Facade area | Stable responsibilities |
| --- | --- |
| `model` | Application identities, workspace specification/view, durable commands, policy configuration, and strict persistence. |
| `geometry` | Logical and physical value types, scale factors, and validated constraints. |
| `runtime` | An opaque core owner, host-frame capability, surface-frame contribution methods, application commands, and a high-level frame report. |
| `presentation` | Read-only core-derived plan records, measurement submission types, output tickets, and presented authority capabilities. |
| `platform` | Typed known/unknown platform facts, opaque current viewport bindings, effect requests, and dispatch-result reporting for a native runtime. |
| `persistence` | Versioned workspace/session snapshots and stable external item-key mapping. |
| `close` | Read-only application close requests, opaque decision tokens, and decision submission. |

Stable model values include `ItemId`, `RootId`, `SurfaceId`, and
`FloatingPresentationId`. Runtime storage identities such as `NodeId`, scene
stamps, reducer ticks, source sequences, and internal generations are not
product-level model identifiers.

The runtime facade should provide typed methods rather than a public universal
input enum. Conceptually, an adapter can begin a host frame, obtain a surface
contribution capability, submit measurements or an explicit unavailable/retained
result, report a presentation observation, submit semantic input backed by a
presented authority, and finish the batch atomically.

The first renderer-neutral runtime slice now exists as
`dockspace::runtime::DockspaceSession`. It privately owns `DockEngine` and
exposes affine application host frames for durable commands, close decisions,
complete uniform measurements, exact paint settlement, and surface-local
pointer input. One public pointer batch carries any number of provider-ordered
edges through edgewise core challenges inside the same rollbackable host frame.
Every edge retains a typed pointer identity, explicit known-or-unknown position,
capture authority, button or terminal reason, and independent receiver facts.
Discrete and phaseful scroll retain device/sequence identities, raw units,
momentum, and exact modifiers; receiver selection and unit conversion remain
core-owned. Paint-time receiver descriptors are not authority: an affine
output capability must first cross an exact final-presentation observation,
after which the facade can bind independently observed framework receiver facts
to the current output. Stale or missing facts degrade to `Unknown` and fail
closed. The independent `dockspace_host_conformance` executor uses this boundary
for `OGC-01` through `OGC-04` without importing engine internals, scene stamps,
provider leases, or the core hit resolver.

Contained geometry follows the same explicit-action rule: passive surface or
minimum-size changes never rewrite durable rectangles, while item-centric
`bring_contained_into_view` may clamp one contained presentation against the
current exact ready surface bounds without implicitly raising it. Unlike
pointer or drop commitment, this explicit application action does not
manufacture or require retained input authority from those geometry facts.

Adapter-only intent, scene-manifest, drop-guide, drop-target, and tab-strip
types are private implementation details. Their former crate-root and backend
module paths are removed rather than preserved as compatibility aliases. The
runtime paint and host-frame DTOs expose only the product facts an adapter needs.

The public `policy` module follows the same split. It contains only declarative
configuration, stable application rule identities, and product-facing axis and
presentation values. Policy revisions, frozen snapshots, evaluator requests,
facts, decisions, and detailed rejections remain private; product setters never
accept the private graph `Axis` type.

The same rule now applies to raw command/transaction errors, the complete
external-item-key map, and low-level viewport generations/bindings. Product
callers use `DockspaceRuntimeError`, `DockspaceDocumentBootstrap`, and opaque
native bindings from `dockspace::runtime`. The underlying error,
external-item-key, and viewport FSM types remain private and their removed paths
are not retained as compatibility aliases.

`DockspaceRuntimeError` also belongs to the facade rather than mirroring the
reducer. Callers inspect only the action-oriented `InvalidConfiguration`,
`Persistence`, `Unsupported`, `OperationConflict`, `HostProtocol`, and
`Internal` categories. Typed interaction and native failures have narrow
accessors, while engine, host-frame, and scene-compilation sources remain in the
standard error chain and outside the default API.

The egui facade follows the same rule. `DockspaceError` is opaque and exposes
only the action-oriented `DockspaceErrorKind` categories: invalid
configuration, persistence, unsupported operation, operation conflict, host
protocol, and internal failure. Exact renderer, pointer, host-frame, and reducer
diagnostics are private implementation details retained through `Error::source`;
there are no public conversions from backend FSM errors into the product error.

Pointer- and semantic-created close plans now return through the same
`HostInputOutcome::CloseRequested` shape as application close requests, with a
small product-level origin instead of a mirrored reducer FSM. Their decision
tokens therefore reach a second adapter, which can veto or allow them through
the same affine host-frame API. Delivery and hover remain independent
event-time routes, but one physical edge asks at most one receiver question;
the runtime does not expose an unreachable combined challenge. Other
interaction terminals remain private until the facade defines equally narrow,
actionable product outcomes for them.

Close publication now follows the same boundary. `HostInputOutcome` carries a
`DockspaceClosePlan` projection and `DockspaceCloseResolution`, not the core
`ClosePlan`, native settlement proofs, lifecycle actions, or the exhaustive
`CloseInertReason` FSM. Applications still receive exact opaque request and
decision tokens, stable target/item identities, policy capability, and the
public phase required to make a decision. Detailed plan authority, native
destruction/cancellation state, and effect correlation remain private; stable
close decisions and target values live under `dockspace::close`.

The primary docking-geometry slice is now renderer-complete. A borrowed
`SurfacePaintPlan` exposes panes, tabs, tab bars, splitters, splitter junctions,
contained presentations, docking-guide clusters and targets, the active
semantic receiver roster, and transient drag preview geometry. Stable opaque
visual identities hide `NodeId`, scene stamps, reducer ticks, and coordinate
generations. The host-conformance executor proves that this geometry roster can
be traversed without reconstructing projection state, that guide clusters
include center and four directional targets, and that visual identities remain
stable across distinct presented outputs.

The native slice adds opaque window and work-area bindings without exposing
provider leases or `ViewportBinding`. An adapter supplies reusable host tokens,
receives core-minted `NativeSurfaceBinding` and `NativeWorkAreaBinding` values,
and atomically validates complete window and explicit exact-or-unknown work-area
rosters. The fixed managed-host profile cannot claim global focus or activation
control, and platforms without exact desktop-global facts must stay on the
surface-local input lane. No public prepared fact can survive a workspace,
binding-incarnation, coordinate-generation, or work-area-generation change.

Desktop pointer input uses product types for event-time hover, physical
position, delivery, capture, button state, scroll phase, and modifiers. A
surface-local pointer provider and the managed desktop provider are mutually
exclusive. Empty host frames do not invent pointer checkpoints. Receiver
resolution is synchronous and asks only for an opaque presented surface or
receiver capability; provider sequence numbers, receipts, and replay remain
private. A delayed A1 binding is rejected before it can act on A2, even when
both use the same host token.

Native failures follow the same boundary: provider leases, recorder invariants,
and exact backend error variants remain private. Product hosts receive only a
stable `NativeHostErrorKind` and retain any affine effect result that could not
be recorded, so retry and cleanup remain possible without freezing the internal
provider FSM as public ABI.

This remains a renderer-neutral coordinator rather than a native product
runtime. OS event-loop scheduling, platform-specific event-time probes,
renderer wakeups, and real-window lifecycle execution belong in the unpublished
native adapter. Rich adapters still need ordered keyboard/accessibility input,
persistence, and one real two-window lifecycle path before they can migrate.

## Internal Implementation Categories

The following current categories are implementation detail after the seal and
must become private modules or private submodules. Read-only views needed by an
adapter are re-exported through the facade instead of preserving these module
paths.

- Reducer and frame orchestration: `engine`, `frame`, `transaction`,
  `transition`, and reducer/event ordering internals.
- Scene construction and resolution: scene compiler/validator, scene sets and
  stamps, drop resolver, drop targets, drop guides, hit regions, and layout
  solvers.
- Interaction state machines: raw renderer intents, drag/resize sessions,
  previews, route proofs, and internal gesture authority.
- Native lifecycle state machines: viewport registry, focus coordinator, route
  state, effect ledger, recovery, retirement, close settlement, and create
  sagas.
- Policy evaluator facts and snapshots: revisions, evaluator requests, and
  internal decisions; public policy remains declarative configuration.
- Raw graph storage and optimistic capture data: node fingerprints, node
  sources, slot-map nodes, canonicalization machinery, and internal validation
  paths.

Opaque platform and effect identifiers must be core-minted. A provider may own
an external window-token value, but it must not mint a core binding incarnation,
effect identity, route generation, or scene authority.

## egui Facade Seal

`egui_dockspace` must not re-export the whole `dockspace` crate. Its supported
public surface is the egui widget facade, builder, pane rendering contract,
style, supported presentation modes, persistence entry points, and high-level
responses.

The following are adapter implementation detail and must not remain public:

- `DockEngine` accessors and core host-frame accessors.
- Host presentation stream, host token, capture-generation, and observation
  bookkeeping types.
- Duplicate egui projection types and `ProjectionError` once the renderer paints
  only the core-derived presentation plan.
- Raw `EngineTransition`, `ClosePlan`, `CommandError`, and
  `InteractionOutcome` values in ordinary widget responses.

Surface-local pointer shutdown follows the same seal. The egui adapter owns a
non-cloneable producer, drains it, and atomically asks core to retire or compact
the exact provider. Ordinary applications do not receive a raw retirement
transition outbox: this lane is forbidden from producing platform effects or
focus obligations, and the adapter consumes only the narrow interaction-change
and repaint requirements before deciding whether its renderer needs invalidation.
Core keeps only a weak lifetime monitor, so an accidentally dropped producer or
drain receipt remains explicitly reclaimable without reopening raw lease-based
submission or retirement.

The current single-surface convenience API remains honest about its scope. A
real multi-viewport host belongs in a dedicated native runtime crate and is not
represented by a public egui callback-order protocol.

The first two breaking slices are complete. The egui whole-crate re-export and
raw engine accessor are gone. Ordinary egui mutation and paint responses now
return product-level command, close, surface, and mutation outcomes instead of
raw `EngineTransition` or surface-contribution FSM values. The renderer-neutral
crate no longer exposes reducer events, transitions, interaction state, hit
regions, or its other backend FSM modules at their former root or backend paths;
adapters use the typed `dockspace::runtime` boundary. Product callers obtain the
opaque, core-minted `WorkspaceVersion` and typed close rejections through
`dockspace::runtime`. Examples and the official-egui harness declare
`dockspace` directly when they intentionally exercise core contracts, so rustc
rather than an API-classification script owns dependency and name resolution.
The low-level backend host response now extracts only facts the native host can
act on: rejected surface registrations, pending platform effects, effect
receipts, native close edges, presentation-settlement counts, and product-level
surface status. It no longer exposes raw transitions or surface-contribution
FSM values. Remaining model consolidation keeps this seal open.

## Conformance Tests

The custom `dockspace_core_protocol` trace schema and direct-engine harness have
been retired. Core invariants remain in crate-local Rust tests, while
product-visible ordering, paint, pointer, close, and observed-root behavior run
through `dockspace_host_conformance` using only the public host-frame facade.
No adapter-conformance crate serializes reducer inputs or imports `EngineInput`,
scene stamps, routes, effect ledgers, or lifecycle FSMs.

## Breaking Migration Order

1. Complete opaque presentation observations and remove public scene-stamp and
   generation leaks from those capabilities.
2. Define the new facade types and migrate egui plus black-box host fixtures to
   them.
3. Remove egui's duplicate projection and replace raw hit processing with
   core-plan views and presented-authority-backed semantic input.
4. Introduce the native runtime contract and migrate platform lifecycle callers
   to typed snapshot/effect APIs.
5. Make legacy modules private, remove root re-exports and compatibility names,
   then publish only the sealed facade.

## CI Guard

The seal needs a small API-surface guard in addition to behavior tests:

- Keep a tiny downstream compile fixture that imports only allowed facade items.
- Build strict public rustdoc and inspect the crate roots during this breaking
  refactor. Once the facade is intentionally stable, an established tool such
  as `cargo public-api` or `cargo-semver-checks` may own a release baseline; do
  not build a repository-specific substitute.
- Keep core-minted constructors private and prove the supported construction
  path with the downstream fixture. Add a compile-fail test only when a
  specific forbidden import is a product contract and no public-item diff can
  express it; do not snapshot broad compiler diagnostics.
- Run adapter conformance through the public host-frame contract, not direct
  `DockEngine` traces.

### Tooling Boundary

Release tooling is an adapter around Cargo, rustc, rustdoc, and the test runner.
It may select manifests, pass feature/configuration flags, verify structured
metadata, compare a checked-in allowlist, and report command failures. It must
not implement a partial Rust front end: no token-based name resolution,
closure/call-graph inference, receiver-type deduction, ABI provenance analysis,
or provider-route intersection is an acceptable substitute for compilation or
runtime conformance. Each claim gets one authoritative gate: dependency source
claims use `cargo metadata`, export claims use `cargo public-api`/rustdoc,
interface usability uses a downstream compile fixture, and behavior claims use
the adapter conformance tests. A script may orchestrate these gates, but must
not duplicate their semantics or manufacture a stronger `Safe`/`Supported`
classification from source text.

Keep runners deliberately boring. Prefer Cargo subcommands and CI matrix
configuration over a repository task framework; do not introduce a task graph,
plugin registry, or shared abstraction for one or two call sites. Executing a
thin runner in CI is normally its test. Add script unit tests only for genuinely
non-trivial, platform-independent argument or structured-data transformation;
if such logic encodes product semantics, move it into Rust and test it there.
