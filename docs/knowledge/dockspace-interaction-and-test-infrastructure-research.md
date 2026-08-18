# Dockspace Interaction and Test Infrastructure Research

Date: 2026-08-18

## Decision Summary

Do not build a general-purpose GUI test engine and do not copy the Dear ImGui Test Engine. The repository should combine `egui_kittest` for egui-local interaction with a thin Dockspace-specific conformance catalog and the existing native coordinator/smoke layers. A reusable adapter driver should be extracted only after a second UI adapter exists.

This split preserves the core rule that Dockspace owns topology, revisions, hit authority, and native lifecycle decisions. Test code may select a product concept such as a tab or splitter, but it must not construct `NodeId`, core receipts, scene stamps, or guessed geometry.

## Scope and source pins

This is a source-based design note, not a claim that physical multiview drag is
already shipping. The repository snapshot is 2026-08-18. The ordinary egui
0.36.1 reference is `repo-ref/egui-release`; the hosted-cycle/native fork is
`repo-ref/egui-release-split` (its current fork commits are visible with
`git -C repo-ref/egui-release-split log`). The Dear ImGui Rust comparison uses
the already-fetched `repo-ref/dear-imgui-rs` object `origin/main` at
`abac30563586`, not that nested checkout's stale local `main`. Keeping these references
separate matters: the fork adds host lifecycle hooks, but it does not turn
egui's logical viewport API into an OS drag-and-drop protocol.

## Sources

The comparison used the checked-in primary sources:

- `repo-ref/egui-release/crates/egui_kittest/README.md` and `repo-ref/egui-release/crates/egui_kittest/src/lib.rs`
- `repo-ref/egui-release-split/crates/eframe/src/epi.rs` and
  `repo-ref/egui-release-split/tests/test_viewports/src/main.rs`
- `repo-ref/egui_tiles/src/behavior.rs`, `repo-ref/egui_tiles/src/tree.rs`, and `repo-ref/egui_tiles_docking/src/container/tabs.rs`
- `repo-ref/dear-imgui-rs` `origin/main:backends/dear-imgui-winit/src/multi_viewport/`
- `repo-ref/imgui_test_engine/docs/README.md`, `repo-ref/imgui_test_engine/imgui_test_engine/imgui_te_context.cpp`, and `repo-ref/imgui_test_engine/imgui_test_suite/imgui_tests_viewports.cpp`
- `crates/dockspace_host_conformance/`
- `integration/egui-product-harness/`
- `integration/egui-native-smoke/`
- `crates/egui_dockspace_native/src/capabilities.rs`

## Existing egui Foundation

The pinned egui 0.36.1 reference contains `egui_kittest` as a workspace crate
(`repo-ref/egui-release/Cargo.toml:10,70`). Its `Harness` owns one
`egui::Context`, one `RawInput` stream, one previous `FullOutput`, one event
queue, and one root viewport (`repo-ref/egui-release/crates/egui_kittest/src/lib.rs:61-90`).
It provides deterministic frame progression, AccessKit tree queries, semantic
actions, keyboard events, pointer events, drag/drop helpers, and optional
low-resolution rendering snapshots (`repo-ref/egui-release/crates/egui_kittest/README.md:8-48`;
`repo-ref/egui-release/crates/egui_kittest/src/lib.rs:244-360,484-607`). It
disables blinking and animation by default so interaction assertions are
repeatable (`repo-ref/egui-release/crates/egui_kittest/src/lib.rs:125-132`).

`drag_at` and `drop_at` only enqueue pointer events in that same context
(`repo-ref/egui-release/crates/egui_kittest/src/lib.rs:602-635`), and viewport
commands read the root viewport (`repo-ref/egui-release/crates/egui_kittest/src/lib.rs:669-756,730-744`).
The harness therefore cannot prove Winit window identity, desktop pointer
position, cross-window capture, monitor work areas, output presentation, or
first-live admission. The fork's `accept_for_headless_host()` is deliberately
an explicit headless acceptance hook (`repo-ref/egui-release-split/crates/egui_kittest/src/lib.rs:851-855`),
not a substitute for native presentation authority.

The current product harness is already a no-backend product fixture
(`integration/egui-product-harness/Cargo.toml:11-18`). It manually supplies
`RawInput`, runs `Context::run_ui`, scans AccessKit, and inspects shapes
(`integration/egui-product-harness/tests/product.rs:158-285,420-469`). It
should selectively adopt kittest-style semantic queries and event sequencing,
while retaining direct `Context::run_ui` tests for multipass discard and
final-presentation settlement. Those tests depend on output ordering that a
generic kittest loop does not model.

The egui project itself recommends ordinary Rust assertions before image
comparison because snapshots are slower, brittle, and expensive to store
(`repo-ref/egui-release/crates/egui_kittest/README.md:95-123`). Dockspace
should follow the same rule: use snapshots only for a small visual contract
such as tab/content connection, guide density, and drag feedback.

The upstream multiview examples are not a ready-made DnD solution. The
viewport test explicitly keeps `DRAG_AND_DROP_TEST` disabled because cross-window
drag-and-drop is not implemented (`repo-ref/egui-release-split/tests/test_viewports/src/main.rs:9-10`).
The example does demonstrate stable `ViewportId`/parent state and immediate
versus deferred callbacks (`repo-ref/egui-release-split/tests/test_viewports/src/main.rs:29-35,64-110`),
while its disabled prototype uses a global dragged-id map
(`repo-ref/egui-release-split/tests/test_viewports/src/main.rs:275-409`). That
prototype is useful as a test sketch only; it would violate Dockspace's
revision-bound, core-owned action authority in production.

## What Makes egui_tiles Feel Seamless

The useful property of egui_tiles is not its `Tree` model. Its interaction
chrome keeps one immediate-mode sequence:

```text
allocate exact rect -> Ui::interact -> Style::interact -> paint -> queue semantic intent
```

The sequence makes hover, press, focus, cursor, and paint use the same egui
state instead of handing the user through a second visual protocol
(`repo-ref/egui_tiles/src/behavior.rs:151-199,229-256`). Tabs use
`click_and_drag`, Grab/Grabbing cursors, a source gap, a pointer-following
ghost, and a selection-stroke preview (`repo-ref/egui_tiles/src/behavior.rs:100-103,172-183,484-516`;
`repo-ref/egui_tiles/src/tree.rs:442-463`).

The docking fork's recent commits make the interaction contract especially
clear: `45e44f1` adds close-hitbox separation, insertion index, and hover-dwell
tab switching; `962eded` prevents a tab button from stealing a background drag;
`ccd2250` and `2d83f0f` make unused tab-bar background a draggable group handle.
The implementation computes background regions while excluding tab and close
rectangles (`repo-ref/egui_tiles_docking/src/container/tabs.rs:449-491`). Those
are presentation/input techniques worth borrowing, not a reason to borrow its
tree mutation.

The current Dockspace facade intentionally consumes the caller's `Ui` rect and
does not insert another panel (`crates/egui_dockspace/README.md:27-40`). The
remaining visual gap is interaction feedback: the tab response uses
`Sense::click_and_drag` but does not yet provide the complete tab cursor/ghost
path, the group handle is only a fixed 28-pixel region, and pane focus has no
production bridge (`crates/egui_dockspace/src/product_render/tabs.rs:149-166`;
`crates/dockspace/src/presentation_config.rs:250-267`;
`crates/egui_dockspace/src/pane.rs:42-67`). These are adapter-facing
improvements, not a reason to reintroduce an egui_tiles tree or a second
topology authority.

The apparent extra panel in screenshots usually comes from a contained/floating
surface, not from a hidden root panel. Its renderer intentionally paints a
floating fill, title fill, and border (`crates/egui_dockspace/src/product_render/contained.rs:14-41`).
That is the right place to converge on egui/egui_tiles visual tokens: use the
current `Ui`'s `Frame`/visuals for radius, stroke, and hover state while keeping
the contained rectangle, raise, move, and resize decisions in core. The pane
itself should continue to receive the caller's `Ui` directly, as egui_tiles'
`Behavior::pane_ui` does (`repo-ref/egui_tiles/src/behavior.rs:90-95`); a second
content panel would make the integration feel disconnected again.

The practical parity backlog is therefore concrete:

| egui_tiles technique | Dockspace adaptation | Boundary |
| --- | --- | --- |
| tab/close disjoint responses | keep opaque core receiver descriptors, create separate egui responses | adapter hit registration |
| Grab/Grabbing cursor | set cursor only for hovered/dragged tab or group region | adapter presentation |
| empty tab-bar drag regions | have core publish exact opaque regions; adapter registers each stable region | core geometry, adapter response |
| source gap and ghost | draw transient decoration from the core gesture, never mutate topology locally | adapter paint |
| hover-dwell tab switch | keep dwell state local for egui-only selection, submit a revision-checked intent | adapter timing + core validation |
| splitter cursor/radius and reset gesture | reuse egui style and add a typed core reset action if desired | adapter feel, core mutation |

Do not let a smooth animated preview replace the exact core preview. Dockspace
already publishes draw/hit/preview/active/rejection data
(`crates/dockspace/src/runtime/paint/guides.rs:61-169`), and the product harness
asserts exact preview settlement (`integration/egui-product-harness/tests/product.rs:1785-1923`).
Only the ghost/border decoration may be interpolated.

The four-way guide should not copy egui_tiles' nearest-zone resolver. Dockspace already publishes separate draw, hit, preview, active, and rejection data (`crates/dockspace/src/runtime/paint/guides.rs:61-169`). Its visible and hit density should be changed through one core presentation preset so decoration, hit bounds, and preview acknowledgement remain coherent.

## What Dear ImGui Test Engine Provides

The Dear ImGui Test Engine demonstrates a useful user-facing vocabulary: named selectors, injected mouse/keyboard input, application-state assertions, optional headless/windowed execution, and bounded screenshot/video capture (`repo-ref/imgui_test_engine/docs/README.md:17-61`). Those ideas are useful for a future Dockspace scenario catalog.

Its docking implementation is not a portable abstraction. `DockInto` directly obtains `ImGuiWindow` and `ImGuiDockNode`, calls the internal `DockContextCalcDropPosForDocking`, teleports windows, hides foreign windows, switches the mouse viewport, and asserts internal fields such as `MovingWindow` and `HoveredWindowUnderMovingWindow` (`repo-ref/imgui_test_engine/imgui_test_engine/imgui_te_context.cpp:4306-4405`). Viewport tests similarly inspect internal viewport pointers and backend callbacks (`repo-ref/imgui_test_engine/imgui_test_suite/imgui_tests_viewports.cpp:65-110,255-347`). The engine license is also not uniformly MIT; the engine directory has a separate license with paid terms for larger businesses (`repo-ref/imgui_test_engine/docs/README.md:65-68`). We should copy the scenario vocabulary, not the implementation, internal oracle, or license.

## What `dear-imgui-rs` Shows About a Native Backend Seam

The fetched `dear-imgui-rs` 0.16 history turns its winit multi-viewport backend
into explicit `runtime`, `registry`, `callbacks`, `coordinates`, `events`,
`focus`, and `viewport_data` owners. The relevant recent commits include
`7fb3308a` (ownership-first contracts), `5a9b4e6e` (identity across contexts),
`d756b97d` (validated native capabilities), `ef28268c`/`7c34d2be` (detached
monitor transactions and degradation), `fd6ba908` (client geometry
reconciliation), and `194cfee2` (event-driven wake). Read them with `git -C
repo-ref/dear-imgui-rs show origin/main:<path>`; do not check out or reset the
dirty nested repository merely to inspect them.

The useful abstractions are narrow:

- One runtime control owner is attached to one ImGui context; a platform
  runtime is a view of that owner, not another owner
  (`multi_viewport/runtime.rs` and `multi_viewport/runtime/lifecycle.rs`).
- `ViewportIdentity` plus the registry route events by actual winit `WindowId`
  and validate context/viewport ownership instead of scanning the current
  viewport list or guessing rectangles (`multi_viewport/registry.rs`).
- Construction, fault, shutdown, and detach are explicit states with staged
  rollback; event-loop access is a scoped borrow that cannot escape the
  callback (`multi_viewport/runtime.rs` and `runtime/lifecycle.rs`).
- Physical/logical client and outer geometry are separate, validated spaces;
  pending client reconciliation waits for a native event before publishing a
  new epoch (`multi_viewport/coordinates.rs` and `viewport_data.rs`).
- Secondary-window input, focus, geometry, and lifecycle events all converge
  on one main-window wake edge, so `ControlFlow::Wait` remains live without a
  busy repaint loop (`multi_viewport/events.rs`).
- Monitor/work-area publication is a prepared detached transaction with
  ownership/provenance and a degradation state rather than several capabilities
  inferred from one rectangle (`multi_viewport/callbacks/monitors.rs`).

Dockspace already has deeper renderer-neutral authority than this backend: one
`DockspaceSession`, exact surface bindings, prepared host frames, output
terminality, and native retirement. It should borrow the registry, geometry
reconciliation, construction rollback, and single wake-edge lessons without
copying Dear ImGui's raw `PlatformIO` pointers, generic attachment/lease/trace
framework, or backend-owned topology.

## Current Dockspace Evidence Layers

The repository already has the beginnings of the right separation:

| Layer | Can prove | Cannot prove |
| --- | --- | --- |
| Core tests | topology, revision, atomic actions, fail-closed semantics | framework widgets or OS windows |
| `dockspace_host_conformance` | public runtime timing, receiver freshness, native protocol facts | egui response behavior or real compositor input |
| Product harness | egui tabs, guides, splitters, contained controls, multipass behavior | OS child windows and desktop routing |
| Native coordinator tests | production-shaped Winit event ordering and lifecycle effects | facts emitted by a real window manager |
| Native smoke | real root/child windows, first-live, ownership transfer, redock, retirement | physical mouse drag; the current tear-off is programmatic (`integration/egui-native-smoke/src/main.rs:126-277`) |

This is stronger than a single universal test runner because each layer has an honest authority boundary.

The current fixture details are worth preserving:

- Core product-action tests prove select/open/dock and root relocation without
  exposing engine node IDs (`crates/dockspace/tests/product_actions.rs:52-120,140-220`).
- Product egui tests already cover tab/accessibility and multipass reorder,
  guides, splitter/junction, contained chrome, and exact discard settlement
  (`integration/egui-product-harness/tests/product.rs:1037-1264,1388-1673,1785-2055`).
- Native interaction-authority tests expose a per-surface receiver result and
  generation, but their helper still renders one surface per invocation
  (`crates/egui_dockspace_native/tests/interaction_authority.rs:41-196`).
- Host conformance deliberately consumes the public session/frame facade and
  checks stale output, receiver freshness, and native facts
  (`crates/dockspace_host_conformance/src/tests.rs:76-137,903-1040`).
- The CI native gate is one bounded Xvfb/Glow lifecycle smoke; it is not an OS
  pointer injector (`.github/workflows/ci.yml:89-163`).

This means a product harness can be green while native multiview remains
unproven, and a native lifecycle smoke can be green while physical drag remains
unreachable. The test names and release documentation should keep those claims
separate.

## Physical Multiview Finding

The example's second window is intentionally created by `request_tear_off_root`;
a physical tab drag is not wired through to a real child-window creation path
(`crates/egui_dockspace_native/examples/basic.rs:149-169`). The native smoke
uses the same programmatic request and then checks first-live, redock, and
retirement (`integration/egui-native-smoke/src/main.rs:126-277`). It is a
valuable lifecycle test, but it must not be described as physical drag E2E.

On macOS, the pinned winit motion path currently lacks the exact desktop
position needed to cross the core drag threshold, and the platform does not
yet provide the complete hovered-window/work-area facts required for an
outside-all release. Windows' declared managed profile also needs its
capture-time hover facts reconciled. Wayland correctly remains unsupported for
this desktop-global contract (`crates/egui_dockspace_native/src/capabilities.rs:14-59`).

The honest production chain is:

```text
Winit event-time facts
  -> ordered native journal
  -> final-presented egui receiver
  -> core-owned gesture and exact preview
  -> CreateWindow effect
  -> hidden/staging/show/Presented
  -> first-live ownership transfer
```

A test engine that injects a core gesture or guesses a window from rectangles would make this chain appear green while skipping the failure the user observed.

The fork's hosted-cycle hooks provide a good deterministic simulator contract:
freeze the complete input roster before any viewport callback, run hidden and
occluded viewports so the output roster stays complete, stage every output,
then publish semantic state only from the commit hook
(`repo-ref/egui-release-split/crates/eframe/src/epi.rs:368-417,434-493`). The
same order should be used by a headless multiview test driver. In particular,
an aborted cycle retains resource-accounting outputs but must not replay its
consumed input (`repo-ref/egui-release-split/crates/eframe/src/epi.rs:450-455`).

## Recommended Conformance Kit

Start with a small Dockspace-specific catalog, not a generic DSL. The shared vocabulary should contain only:

- selectors: `Tab(item_key)`, `Close(item_key)`, `Splitter(path)`, `DropZone(direction)`, and `Surface(alias)`;
- actions: click, drag, keyboard, scroll, and AccessKit action;
- observations: selection, item/root/surface ownership, normalized layout, terminal action status, presented/retired surface state, and quiescence;
- capabilities: single-surface pointer, accessibility, native lifecycle, global pointer routing, cross-window routing, mixed DPI, and real OS windows.

An adapter maps these concepts to its own AccessKit tree, widget state, or native facts. The kit must never expose scene stamps, receipts, core hit resolvers, raw graph IDs, or paint-plan coordinates as test inputs.

The first implementation can remain ordinary Rust functions in `crates/dockspace_host_conformance` plus focused egui/native harness tests. Extract a small driver trait only when a second UI adapter needs to run the same scenarios; before that point a public trait would merely freeze an unvalidated abstraction.

## Automated Multiview Test Shape

The smallest useful automated multiview proof is a headless native-host
simulator, not a second reducer and not an OS-coordinate macro. One cycle
should carry:

1. A stable surface/viewport alias and generation for every root and deferred
   child.
2. A frozen input roster containing event-time window identity, hover, capture,
   desktop position, focus, and scroll challenge facts.
3. Per-surface output in actual generation order, including the final receiver
   manifest and presentation token.
4. A terminal preflight/settlement step followed by one semantic commit.

The driver action sequence should look like the ImGui engine's public test
choreography (`repo-ref/imgui_test_engine/imgui_test_engine/imgui_te_context.cpp:3429-3507`): resolve a semantic source,
press, advance frames across the drag threshold, move to an explicitly selected
target viewport, assert the preview before release, release, and then assert
the typed action outcome and ownership. The Dockspace version must submit only
through the public product/native facade; it must never call `DockInto`, inject
a core gesture, or derive a target from a cached rectangle.

The first multiview simulator cases should be:

| Sequence | Assertions before release | Assertions after release |
| --- | --- | --- |
| tab source -> same-surface reorder | source gap, insertion preview, focus remains on source | order and revision receipt |
| tab source -> child viewport | target receiver and exact guide preview | typed tear-off action, child first-live, item ownership |
| child -> root redock | stale/foreign target rejected or previewed | root ownership, child retirement, quiescence |
| close/viewport destroy during drag | capture cancellation and no old receiver reuse | no stuck button/capture ledger, typed cancellation |
| click/drag/scroll on different regions | lane-specific receiver and scroll challenge | only the selected lane is delivered |
| discarded/superseded pass | old output cannot release presentation freshness | resource settlement without semantic publication |

The simulator should model at least two surfaces in one hosted cycle and run a
hidden/occluded child callback. It should use stable semantic IDs, not fixed
pixel coordinates. A later platform smoke may translate a diagnostic receiver
center to real OS input, but that is a separate proof.

## Recommended Private Seams and Anti-Patterns

The interaction work can be modularized without changing the public product
facade:

- `egui_chrome::interaction`: stable, disjoint Responses; `Sense`; cursor and
  `Style::interact` mapping.
- `tab_strip`: tab/close responses, background drag regions, overflow, and
  hover-dwell state.
- `drag_feedback`: source gap/dim, ghost, and exact-preview decoration.
- `splitter`: pointer/keyboard/AccessKit actions and optional double-click reset.
- `contained_chrome`: egui `Frame` recipe plus title/close/resize responses;
  durable rect and move/resize action remain core-owned.
- `focus_bridge` and `accessibility`: pane focus requests and shared TabList /
  TabPanel roles.
- `host/output_batch`: one native cycle's roster, output order, texture and
  presentation settlement.

The following patterns would make the current problem worse and are explicitly
out of scope:

- Do not import `egui_tiles::Tree`/`Behavior` as a second topology or active-tab
  authority. `Tree::ui` mutates and simplifies its own graph
  (`repo-ref/egui_tiles/src/tree.rs:304-423`); Dockspace's core must remain the
  only graph owner.
- Do not let the egui adapter calculate a new drop target from pointer pixels;
  use the core's opaque guide/hit/preview records.
- Do not use ImGui Test Engine's privileged `DockInto`, `DockClear`, or
  `WindowMove` as production or conformance actions
  (`repo-ref/imgui_test_engine/imgui_test_engine/imgui_te_context.cpp:4301-4417`).
- Do not expose `NodeId`, scene stamps, receipts, raw paint-plan geometry, or
  window handles merely to make a test selector convenient.
- Do not use fixed-coordinate `xdotool`/screen macros as a cross-platform proof;
  they bypass event-time hover/capture/work-area facts.
- Do not call `request_tear_off_root` from a physical-input test as a fallback;
  that proves a programmatic lifecycle, not a drag.

## Scenario Matrix

| Scenario | Required evidence |
| --- | --- |
| tab select/close/reorder, keyboard, AccessKit | egui kittest/product harness |
| splitter/junction and contained move/resize | egui product harness plus core conformance |
| guide hover/active density and exact preview acknowledgement | egui product harness plus core conformance |
| click/drag/scroll lane separation and Unknown fail-closed | host conformance plus native coordinator tests |
| outside-all release and child creation | native coordinator plus real-window smoke |
| first-live barrier and ownership transfer | host conformance plus real-window smoke |
| cross-window redock, close during drag, ABA, scale/work-area staleness | host conformance plus native coordinator tests |
| compositor-generated physical pointer drag | separate, platform-specific pointer smoke only where capabilities are complete |

### Proposed Test Tiers

| Tier | Fixture | Release claim |
| --- | --- | --- |
| 0 | core unit/model and geometry tests | topology, revision, action atomicity, guide and splitter math |
| 1 | core property/state-machine tests | ordered pointer journal, cancellation, stale generation, quiescence invariants |
| 2 | public host conformance | output freshness, receiver route, native roster, typed receipts without engine access |
| 3 | no-backend egui product harness, optionally kittest-style | same-surface tab/guide/splitter/contained interaction, AccessKit, multipass and visuals |
| 4 | native coordinator + headless multiview simulator | multi-surface cycle ordering, event-time route, first-live barrier and redock semantics |
| 5 | one real-window lifecycle smoke | actual OS child creation, first-live, ownership transfer, retirement/quiescence |
| 6 | platform-specific physical pointer test/manual gate | only after the platform supplies exact desktop position, hover/capture, work area, and input injection |

Tier 3 should remain the default product acceptance path. The follow-on plan
adopts `egui_kittest` selectively for semantic lookup and deterministic event
stepping while retaining the current direct multipass hooks; it does not replace
the harness wholesale. Tier 4 is the missing automated multiview proof.
Tier 5 must keep its current narrow name, and Tier 6 must be opt-in per platform
until the capability matrix is truthful.

## Decision and Plan Trigger

The companion follow-on plan at
`docs/plans/2026-08-18-001-interaction-floating-multiview-conformance-plan.md`
is the appropriate execution boundary. The August 8 product-boundary plan
established the authority model and native lifecycle seam; this research adds a
distinct interaction-parity and evidence strategy with several coupled
decisions: how to borrow egui_tiles' feel without a second graph, where to use
egui_kittest, how to keep adapter-neutral scenarios typed, and how to gate
physical multiview claims by platform facts. The plan should continue to
exclude a generic GUI engine, an imgui_test_engine port, screenshot-as-oracle
testing, and Open-GPUI integration.
