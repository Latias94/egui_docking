---
title: Dockspace engine host architecture and direct manipulation - Plan
type: refactor
date: 2026-08-21
revised: 2026-10-29
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
review_status: independent-coherence-and-feasibility-reviewed
---

# Dockspace engine host architecture and direct manipulation - Plan

## Goal Capsule

| Field | Contract |
| --- | --- |
| Objective | Deliver `dockspace` as a renderer-neutral docking library for a custom game engine's self-drawn editor UI, with mature local and contained interaction and an optional native multiview host contract. |
| Primary consumer | An engine owns its UI tree or display list, renderer, frame scheduling, input delivery, and platform windows. It integrates through documented public `dockspace` APIs without egui. |
| Validation | A small external Rust consumer proves host independence; egui is the maintained interactive reference adapter. ImGui docking supplies architectural and behavioral comparisons. |
| Authority | Core owns topology, policy, selection, target resolution, transient layout, terminal mutation, persistence identity, and native transfer semantics. Hosts own framework input facts, actual output disposition, and OS or renderer resources. |
| Execution profile | Breaking replacement. Close the current correctness defects, establish the engine host contract, modernize egui, then converge interaction and optional native lifecycle owners. |
| Stop conditions | Reject duplicate topology owners, renderer-selected targets, traversal receipts presented as GPU proof, lost input boundaries, stale output authority, and capabilities inferred from missing facts. |
| Completion owner | Each implementation unit owns its replacement behavior, meaningful verification, obsolete-path deletion, and documentation. No compatibility layer is required. |

---

## Product Contract

### Summary

`dockspace` is the product; `egui_dockspace` and its native integration are reference consumers.
The core must support both immediate consumption of an ordered surface program and an engine compiling that program into its own display list or retained UI nodes.
The engine need not adopt egui's widget identities, multipass layout, callback lifecycle, graphics backend, or window manager.

The existing stable Item/Root/Surface model, validated layout mutations, measurement separation, and versioned persistence remain useful foundations.
The refactor removes duplicate gesture terminal paths and framework assumptions while retaining exact output identity, stale rejection, cancellation, and recoverable native transfer.

This revision changes the product priority of the earlier experience plan in place.
It preserves R1-R18 and U1-U12 identifiers while revising their host and delivery contracts, adds host requirements R19-R24, separates layout-shell requirement R25, and adds units U13-U15, and leaves implementation progress in Git and `docs/VALIDATION.md`.
The earlier product-boundary plan remains context for persistence and semantic ownership; this plan governs the current host, interaction, and delivery priorities.

### Problem Frame

A renderer-neutral dependency graph alone does not establish a usable engine integration.
Current rendering entry points borrow an active frame, some necessary host APIs are hidden, SourceGap text prescribes widget registration, and the default API requirements conflate application code with renderer-host code.
Input and release decisions also remain distributed across local, journal, and contained reducers.

The native adapter currently embeds framework-specific output, hidden-stage, receiver, and retirement maps around the core lifecycle.
The engine needs the semantic lifecycle guarantees, but should not implement eframe's precise sequence of callbacks and staging passes.
The previous emphasis on an egui product release does not adequately test this distinction.

### Key Decisions

- KD1. Keep this as the canonical interaction and host architecture plan, with the earlier authority and conformance plans retained as historical context. Governs R1-R25.
- KD2. Preserve the stable core topology and transaction authority. Borrow ImGui behavior and platform/renderer separation without importing its binary tree, global context, or per-frame window lifetime model. Governs R2, R9-R15, R19-R22.
- KD3. Permit breaking replacement and delete superseded APIs, reducers, and sidecars with each completed replacement. Governs R9-R16, R19-R24. (session-settled: user-directed - chosen over compatibility layers: correctness and clean ownership take priority)
- KD4. Deliver local and contained engine use before requiring native platform certification. Native multiview remains a core capability with a later readiness checkpoint; individual platform claims require their own evidence. Governs R6, R13-R15, R18-R20.
- KD5. Use one magnetic target solver. Guide icons explain its highest-priority explicit fields; arrangement lock is a separate policy. Governs R3, R4, R7.
- KD6. Make host evidence explicit and framework-neutral. An adapter may advertise only the output and hit authority it can actually supply; an egui limitation does not become a universal core limitation. Governs R11, R12, R16, R19-R22.
- KD7. Make the custom engine host the primary integration audience and egui the interactive validation adapter. Governs R16, R18-R24. (session-settled: user-directed - chosen over egui-first product sequencing: the intended deployment is self-drawn game-engine editor UI)
- KD8. Defer Open-GPUI adapter development and cutover. Its historical algorithms remain optional reference material and do not gate this plan. Governs R18, R19, R23. (session-settled: user-directed - chosen over parallel adapter work: prioritize dockspace and egui validation)

### Requirements

#### Direct manipulation

- R1. A visible tab body is an ordinary drag entry. Below-threshold release selects; crossing the threshold begins one gesture. Ordered press, move, and release boundaries survive delivery within one host tick. Close and group-drag regions remain disjoint.
- R2. Reorder, merge, group drag, center docking, edge split, cancellation, stale rejection, and focus recovery use one semantic manipulation model. Center merge preserves relative item order.
- R3. Core owns magnetic acquisition, priority, latch, hysteresis, and deterministic ties. Explicit guide and tab-gap hits outrank an outer-root band, then the deepest eligible pane basin. A rejected front candidate never silently falls through to another target.
- R4. Release uses the logical latch. A matching current presented preview may commit once; an unpresented preview freezes a pending release until its exact output is accepted as current presentation under R11. Discard, invalidation, or cancellation cannot commit it. No-latch release inside an application surface uses contained placement; outside-all release requires R6.
- R5. Contained move and eight-direction resize project chrome, tab strip, pane content, hit geometry, and semantics together. Durable placement changes only at terminal commit. The active source projection does not hide eligible targets behind it.
- R6. Native promotion requires complete facts for the chosen input lane, target inventory, placement/work area, lifecycle, and release. Missing native facts produce a typed pending, unsupported, or cancelled outcome and preserve a recoverable source; they never silently choose another presentation mode.
- R7. Core exposes common-path semantic commands for select, close, directional dock, float, and dock back. Adapters provide discoverable chrome; the later layout-management shell is R25. Tab, group, contained title, and close controls have one semantic owner; Close Group and Dock Back are distinct operations.
- R8. Manipulations expose typed pending and terminal outcomes, stable deduplication identity, disabled reasons, and semantic focus intent. Hosts choose the visual status surface and accessibility bridge. Feedback never steals focus or replaces pane content; live motion does not announce each pixel.

#### Interaction, output, and optional native lifecycle

- R9. One private manipulation kernel owns arming, sequence, source, lane, target latch, dwell, transient projection, release barrier, cancellation, and terminal result. Framework response adapters and platform journals normalize facts without owning separate terminal reducers.
- R10. Core emits one ordered hierarchical surface program for geometry, clipping, content slots, semantic controls, source gaps, previews, guides, menus, and feedback. It is a semantic composition contract rather than an egui widget or graphics API. Paint, hit, and semantic identities derive from the same snapshot.
- R11. Program acceptance and current presentation are distinct. Successful complete traversal proves emission to an accepting host, not GPU submission. A host reports one exact output's terminal presented or dropped result at the boundary it owns; core validates and commits that observation before promoting interaction authority. Submission alone does not imply presentation, and physical scanout is not claimed. Discard and unwind abandon the exact output. There is no public preview-only acknowledgement bypass.
- R12. Hosts supply typed docking-hover answers for a specific accepted output: candidate, blocked, no receiver, or unknown. Ordinary pane content may pass this lane while popup, modal, foreign overlay, explicit blocker, and ambiguous identity stop it. Framework hit-stack inspection is adapter work; core does not implement an external widget-tree resolver.
- R13. Buffered host input uses bounded, causal, retryable batches under the resource budgets. Idle motion may coalesce before sequencing; active gesture edges and their ordering are preserved. Output, lifecycle, capture, contact, scroll phase, focus, close, inventory, and capability boundaries are barriers.
- R14. Surface-local and desktop-global input have explicit scopes. A gesture freezes its admitted lane; changing providers or scope requires validated continuity of sequence, device/contact, held buttons, capture, and delivery. Local operations never require unavailable desktop-global facts. egui local responses remain adapter evidence, not a distinct core transaction kind.
- R15. One core native runtime owns desired viewport bindings, pending transfer, admission, close, recovery, and retirement by incarnation. The host owns OS and renderer resources. The source retains durable ownership until the child is ready with its exact first valid interactive output; core then publishes transfer once. Hosts may use hidden staging internally, but fixed PreShow/PostShow rendering passes are not universal core requirements. Late predecessor facts cannot affect successors.
- R16. Public integration has two audiences. Application facades hide host plumbing; engine hosts receive documented surface registration, measurement, input, program execution, output disposition, focus/repaint, and optional native-effect APIs. Layout and semantic-command use remains valid without enrolling pointer or native services. Internal graph nodes, reducer internals, provider bookkeeping, and arbitrary target/preview constructors remain private. Required host methods are not hidden documentation seams.
- R17. Pointer, keyboard, menu, and accessibility actions converge on the same prepared commands. Disabled or occluded controls cannot execute. Dwell never guesses pane focus. A SourceGap preserves source identity and explicitly retained semantic behavior; widgets and AccessKit registrations are adapter implementations.
- R18. Acceptance includes semantic journeys through an independent public consumer and egui. Model, recorded-output, real-window, and physical-input evidence are labelled separately. Queues and lifecycle state reach bounded quiescence. Platform capability-negative results do not certify physical interaction.

#### Engine host and dependency independence

- R19. An ordinary external Rust consumer can integrate using only public `dockspace` and application types. Default and persistence-enabled core dependencies and public types exclude egui, eframe, AccessKit, Winit, WGPU, and GPUI.
- R20. A logical Surface is a core layout/presentation boundary, not an OS-window identity. Hosts may use multiple surfaces within one window or render targets, and may bind surfaces to native windows when supported. Local routing, logical coordinates, desktop placement, framebuffer density, and DPI mappings remain explicit rather than inferred from each other.
- R21. Hosts can compile a borrowed surface program into their own owned display list or retained-node update, release the frame borrow, and execute that output later. The opaque output identity survives without cloning internal authority. Superseded, dropped, or retired output cannot promote its receivers.
- R22. The core requires no widget tree, font shaper, graphics device, event-loop implementation, or wall-clock polling. Hosts supply measurements, monotonic event times, typed blocker facts, output disposition, and wake/focus execution. Cached valid measurements are allowed; a new asynchronous layout framework is not required.
- R23. Maintain egui validation on a pinned current stable release and a matching minimal fork where necessary. Core architecture and readiness do not depend on retaining a particular egui fork. egui_tiles remains reference-only; Open-GPUI development is deferred by KD8.
- R24. Host portability and structural cost are verified before declaring core readiness. Immediate and deferred consumption, two logical surfaces without native windows, bounded work at 16/128/1,024 panes, and exact stale-output rejection use the same public host contract.

#### Layout-management completion

- R25. The later editor shell provides closed-item inventory/reopen, arrangement lock, reset/default layout, application-owned named presets, complete overflow ownership, and restore-time focus/missing-pane recovery. These commands use the same core authority and are completed by U10; they do not block U12 common-path readiness.

### Actors and Key Flows

- A1. Engine/application authors own pane data, drawing, UI objects, event loop, platform resources, and policy decisions.
- A2. `dockspace` owns semantic and durable docking state and produces typed host work.
- A3. egui and native adapters translate framework facts and execute host work.
- A4. Independent-consumer, product, and physical executors supply evidence at their stated strength.

- F1. Host supplies ordered input and current measurements; core reduces one gesture and compiles a surface snapshot; host accepts its program and retains the output identity. Covers R1-R5, R9-R12, R19-R22.
- F2. Host finalizes or drops that output; a core commit accepts the exact observation; receivers and a pending release advance together. Covers R4, R11, R18, R21.
- F3. Engine compiles records to owned UI/render work, ends the frame borrow, and later submits or drops its output without retaining internal graph access. Covers R10, R16, R19-R22.
- F4. Optional native create/ready/first-live either transfers the source once or recovers it, followed by close and retirement. Covers R6, R14, R15, R20.
- F5. Application close decisions produce ordinary semantic commands; the later default/preset shell extends them under R25. Adapters render status and focus intent. Covers R7, R8, R16, R17, R25.

### Acceptance Examples

| ID | Given / When | Required outcome | Covers |
| --- | --- | --- | --- |
| AE1 | Drag over a real text editor, scroll region, button, or canvas to three non-guide points in each basin | Correct target preview and one commit; actual child-widget bounds establish the input point | R1-R4, R12, R18 |
| AE1b | A host cannot provide exact target-side blocker facts or terminal output disposition | Typed missing capability and unchanged source; unrelated local operations still work | R6, R11, R12, R14 |
| AE2 | Acquired edge target jitters and releases within its hysteresis margin | The matching latched candidate wins without adjacent flicker | R3, R4 |
| AE3 | Move or resize an overlapping contained inspector | Full pane subtree, hit data, and semantic bounds follow; one durable terminal result | R5, R10 |
| AE4 | Release before the output owner finalizes its preview, including an entire gesture in one tick | Pending release commits only after exact presentation acceptance, or cancels on drop | R1, R4, R11 |
| AE5 | The final output is discarded, superseded, fails, or unwinds | No preview-only ACK can authorize a mutation; prior valid presentation remains consistent | R4, R11, R21 |
| AE6 | Create or replace a child viewport while old callbacks arrive late | Single owner, first-live transfer, recoverable failure, and no successor corruption | R6, R15 |
| AE7 | Sustained high-rate input or uncompressible overload | Ordered accepted edges, bounded queues, explicit terminal overload, and recoverable source | R13, R18 |
| AE8 | Close tab/group/contained root or choose Dock Back | Exactly the named operation and close policy execute, with no overlapping control owner | R7, R17 |
| AE9 | Splitter, keyboard, accessibility, and local touch manipulate a layout | Shared command semantics, rollback on loss/cancel, and ordinary pane scrolling before touch arming; locked-layout cases are certified with U10 | R7, R9, R17, R25 |
| AE10 | Close, dock, reopen, restore, or reject a manipulation | Stable focus successor and one typed feedback outcome without focus theft; reopen/restore is certified with U10 | R8, R17, R25 |
| AE11 | A platform lacks required cross-window facts | Its native claim stays unavailable while proven local/contained operations remain usable | R6, R14, R18 |
| AE12 | An external crate uses only public `dockspace`, fixed metrics, and a recording renderer | Local journeys run with no UI-framework dependency or private test imports | R16, R19, R22, R24 |
| AE13 | Compile to owned records, drop frame borrows, then accept outputs out of order | Deferred output works; obsolete output cannot replace the current receiver artifact | R11, R21, R24 |
| AE14 | Route two logical surfaces in one host window and vary framebuffer density separately from DPI | Correct local coordinates and targets without enrolling native lifecycle | R14, R20, R24 |

### Success Criteria and Resource Budgets

| Signal | Exit criterion |
| --- | --- |
| Host independence | The public consumer satisfies AE12-AE14 with default and serde core configurations and no UI/platform framework dependency. |
| Ordinary docking | Five basins x three non-guide samples x four cadences pass in public-host and egui executors; the Burst row is not ignored. |
| Contained manipulation | Full content/hit/semantic projection during move and eight-direction resize; one terminal mutation. |
| Blocker correctness | Actual child-widget bounds and popup/modal/overlay/front-contained cases verify R12. |
| Structural cost | At 16, 128, and 1,024 panes, counted compiler/resolver work and recorded retained-state size do not regress into repeated full-workspace scans per operation. Native queue budgets are measured separately. |
| Optional native readiness | Programmatic lifecycle, failure/replacement/close cases, and at least the qualified X11 physical reference journey prove the native claims made. |
| Platform claims | Each OS/backend claim has its own deterministic, real-window, and physical evidence; unsupported facts remain explicit. |

The following are initial buffered-input/native-host release budgets, not a requirement for the engine to adopt a particular event loop or allocate a journal when it has no buffered ingress.
Use one injected monotonic time source for age and latency; deterministic tests use controlled time.

| Resource | Budget | Measurement boundary |
| --- | --- | --- |
| Active ingress journal | 4,096 records, 4 MiB resident owned data, 250 ms oldest-record age | Fixed envelope plus owned payload capacity; shared sidecars charged once by their owner. |
| One reduction batch | 256 records or 2 ms, whichever comes first | From immutable prefix claim to reducer yield; barriers terminate a batch and the next claim resumes at its exact ordinal. |
| Release latency | 100 ms under a 1,000 Hz active-input profile | Accepted release edge to typed terminal outcome, excluding an explicitly pending OS create or unavailable renderer. Those outcomes remain visible and cancellable. |
| Retained lifecycle detail | 64 recent terminal summaries per binding and 1,024 monotonic IDs per session watermark | No authority-bearing geometry or receipts in terminal summaries. |
| Lifecycle soak | 10,000 cycles, zero active obligations after quiescence | Bound output, receiver, route, reservation, effect, timer, and retirement state. |

Each active buffered stream reserves an overload terminal latch outside ordinary capacity.
After a cap is crossed, ordinary ingress freezes, accepted predecessors are consumed in order, then overload cancels and restores the last valid source.
The overload path must not allocate another ordinary record or discard an accepted release/capture/lifecycle barrier.

### Scope Boundaries

In scope: core public host contract, ordered semantic programs, local and contained interaction, optional native lifecycle semantics, egui validation and dependency modernization, persistence/layout commands, meaningful conformance, and removal of obsolete paths.

Deferred: Open-GPUI adapter work; cloud/collaborative layouts; physical cross-window touch/pen until their facts are proven; automated Windows/macOS physical infrastructure when no stable environment is available.
Windows/macOS certification remains U11 and gates their capability claims, not generic core readiness.

Outside the product: an actual game engine, ECS integration, a font or widget system, a graphics API, an autonomous window manager, a universal GUI automation DSL, compile-time source scanners, and a security boundary against a dishonest host.
The host is trusted to report real facts; exact identity and ordering reject accidental stale, foreign, or contradictory reports.
No zero-allocation, `no_std`, C ABI, plugin ABI, or arbitrary render-thread scheduling framework is introduced without a concrete consumer requirement.

---

## Planning Contract

### Key Technical Decisions

- KTD1. Separate the application facade from the documented engine-host facade under R16. Keep `DockspaceSession` as the integration boundary and deepen its existing modules before adding another public controller. Implements KD7. (session-settled: user-directed - chosen over an egui-shaped universal facade: engine hosts are the primary consumer)
- KTD2. Keep `SurfaceProgram` as the semantic composition protocol under R10/R21. Input, cancellation, focus and wake processing occur through host input/effect ports rather than as side effects of painting feedback records. A host can consume it immediately or compile records into owned work; batching may preserve compositing semantics but cannot reorder overlapping operations or detach hit semantics from their snapshot.
- KTD3. Put gesture lifetime and terminality in one private module under R9. Move old local/journal/contained decisions into it and delete their independent release and preview-ACK paths.
- KTD4. Keep semantic target acquisition in core under R3. Host hit queries authorize an exact semantic candidate or block it; renderers never choose another target after a rejection.
- KTD5. Preserve the single magnetic profile under KD5. Configure logical distances and dwell in core; adapt cursors, visuals, touch affordances, and accessibility in hosts.
- KTD6. Make the existing presentation pipeline the only current-output promotion path under R11. Execution receipts bind exact attempt/output/contribution/interaction but attest only complete emission. Adapter pass bridges normalize framework finalization and feed the same terminal reporting path; no generic GPU fence or pixel-visibility proof is added.
- KTD7. Separate causal ingress from per-incarnation host resources under R13/R15. Core owns semantic native transfer; the host's runtime joins output, receiver, resource, staging, no-frame, and retirement records without owning a second dock graph.
- KTD8. Support logical-surface input before desktop promotion under R14/R20. Validate handoff only when scope/provider changes, preserving one gesture and one terminal release rather than creating parallel sessions.
- KTD9. Extend the finite shared journey catalog under R18/R24. Add one normal public consumer with a recording renderer, not a new generic GUI DSL or independent target resolver.
- KTD10. Use replacement deletion gates under KD3. No unit completes with old and new terminal owners both callable.
- KTD11. Keep content data, font shaping, draw resources, and named-preset storage application-owned under R7/R22. Core owns stable item identity, close decisions, command results, and versioned docking documents.
- KTD12. Measure boundedness at the owner of each queue or retained resource under R13/R24. Deterministic structural counters cover algorithmic work; host measurements cover wall-clock cost and latency.
- KTD13. Implement R12 as a host port. An engine can answer from its own composed UI; the egui fork may expose a narrow final-pass stack query. Portable egui must expose its exact unavailable boundary rather than approximate popup/modal authority.
- KTD14. Use `integration/dockspace-host-harness/` as an excluded, ordinary downstream workspace depending only on public `dockspace` and small test dependencies. Its recording renderer owns records after the frame borrow and uses the same output reporting contract as real hosts. Implements KD7 and R19/R21/R24.
- KTD15. Upgrade the official egui adapter to 0.37.0 as a separate change under R23. Rebuild only admitted fork seams on the same upstream release before advancing native certification; do not merge stale experimental fork branches wholesale.
- KTD16. Keep first-live as an engine-implementable readiness condition under R15. Distinguish desired binding, renderer/window readiness, durable item ownership, and recovery resources; fixed hidden-stage passes are host strategy rather than universal core phases.

### High-Level Technical Design

```mermaid
flowchart TB
  App[Application pane data and policy] --> Core[Dockspace session and commands]
  Input[Host ordered input and hit facts] --> Kernel[One manipulation kernel]
  Metrics[Host measurements and coordinate facts] --> Core
  Kernel --> Core
  Core --> Compiler[Core semantic surface compiler]
  Compiler --> Program[Ordered program and exact output identity]
  Program --> Engine[Engine display list or retained nodes]
  Program --> Egui[egui validation renderer]
  Engine --> Owner[Host output owner]
  Egui --> Owner
  Owner -->|exact terminal result| Pipeline[Core presentation pipeline]
  Pipeline -->|committed current snapshot| Kernel
  Core -->|optional effects| Native[Host platform and renderer services]
  Native -->|typed readiness and lifecycle facts| Core
```

```mermaid
sequenceDiagram
  participant H as Engine host
  participant C as Dockspace
  participant R as Host renderer
  H->>C: Ordered facts and valid measurements
  C-->>H: Borrowed semantic program and output identity
  H->>H: Compile into owned records
  H->>C: Complete emission and commit host frame
  Note over H,R: Frame borrow is released; host controls scheduling
  H->>R: Submit owned output
  alt owner finalizes exact output as current
    R-->>H: Host terminal disposition
    H->>C: Report exact presented output
    H->>C: Commit observation
    C-->>H: Current receiver artifact and pending gesture result
  else output dropped or superseded
    H->>C: Report exact dropped output
    C-->>H: No new receiver authority or release commit
  end
```

```mermaid
stateDiagram-v2
  [*] --> Idle
  Idle --> Armed: ordered press
  Armed --> Dragging: threshold crossed
  Armed --> Idle: selection release
  Dragging --> Latched: eligible candidate
  Latched --> ReleasePending: release before matching presentation
  Latched --> Committed: release with matching presentation
  ReleasePending --> Committed: matching output accepted by core
  ReleasePending --> Cancelled: dropped stale blocked or lost
  Dragging --> NativePending: qualified outside-all release
  NativePending --> Committed: first valid child output and transfer
  NativePending --> Recovered: failed create close or interruption
  Dragging --> Cancelled: cancel or authority loss
  Committed --> Idle
  Cancelled --> Idle
  Recovered --> Idle
```

### Host Boundary

| Host supplies or executes | Core supplies or owns | Kept out of core |
| --- | --- | --- |
| Surface bounds, scale mappings, text/chrome metrics, pane minima | Layout requirements, deterministic geometry, measurement validity | Font shaping, DPI guesses, renderer resources |
| Ordered pointer/contact/keyboard and target-side blocker facts | Gesture sequencing, target policy, cancellation, prepared commands | egui Response lifecycle, raw OS callbacks |
| Accept or compile semantic program, preserve content slots | Ordered composition, identities, clip/hit/semantic records | Galley, AccessKit nodes, vertices, render graph |
| Terminal output disposition and wake/focus execution | Current presentation and exact receiver identity | Output-shape parsing, GPU fences, host scheduling policy |
| Optional create/destroy/place/show and renderer readiness | Desired native lifecycle, pending transfer, close and recovery semantics | OS handles, swapchains, mandatory eframe staging choreography |

### Default Interaction Profile

These defaults implement R1/R3/R5/R7/R8/R17; they remain core logical units rather than egui pixels.
Hosts may style the controls and supply measured dimensions without changing target priority or terminal rules.

| Behavior | Default |
| --- | --- |
| Drag arming | 6 logical-point threshold; below-threshold release selects |
| Magnetic center | Central 52% by 52% of the target content rectangle |
| Other directions | Normalized nearest edge outside the center; outer-root band is 15% of the short side, clamped to 32-72 logical points |
| Target stability | 8 logical-point release hysteresis with deterministic switch advantage |
| Hover selection | Optional 150 ms event-time dwell, reset by target/semantic scene/gesture/stream/policy change; same-geometry emissions preserve the origin, execution needs current output proof; never implies pane focus |
| Temporary suppression | Shift disables magnetic acquisition in the default editor profile; R25 arrangement lock remains independent |
| Local touch | 350 ms long press and 8-point slop; pre-arm movement yields to pane scrolling, early release activates/selects, armed stationary release opens a semantic context menu |
| Resize and controls | Eight disjoint contained directions; existing mouse/pen/unknown geometry stays independent. Explicit Touch requests at least 44 logical points for resize targets before surface, root, protected-chrome and occlusion clipping; junctions outrank ordinary handles and final same-stack ambiguity is rejected |
| Splitter commands | 16-point keyboard/accessibility adjustment; double-click equalizes only the adjacent pair while preserving their combined share |
| Overflow completion | Under R25, one owner at a time: selected-tab reveal, manual scrollbar, active-drag edge scroll, or menu selection; clipped subcontrols cannot execute. Local edge scroll uses a 24-point content band capped at half-width, a 50ms interval and one scroll-line step per current-output continuation |

Focus intent under R8/R17 uses the following successors.
Native window focus and pane-content focus still require actual host facts and are not inferred from semantic selection.

| Action | Semantic successor |
| --- | --- |
| Select | Selected tab; pane-content focus changes only through explicit request/observation |
| Reorder or dock | Moved item; retain intent until its matching presented semantic node exists |
| Close tab | Next visual tab, otherwise previous tab, group handle, then root workspace |
| Close group | Previously active destination/root item when available, otherwise root workspace |
| Popup dismissal | Invoking tab/group/control, or the relevant close/reset successor if it disappeared |
| Reopen, reset, preset restore | Under R25, reopened item or prior stable focused item when present, otherwise primary selection/root workspace |
| Pending, rejected, recovered, or fatal status | No focus change; announce the inert status once |

### Dependency and Fork Adaptation

The 2026-10-27 selection audit chose egui/eframe 0.37.0, released 2026-10-15,
with Rust 1.95 still matching this workspace. The current workspace and official
harness now pin the matched 0.37.0 family. The optional native adapter is locally
qualified against the rebased, unpublished 0.37 fork; its tracked pins still name
the earlier fork and are not evidence that a clean checkout can reproduce the
new native seams. Winit remains on the reviewed 0.30.13 fork; prerelease 0.31 work
is not a prerequisite. The updated local egui_tiles checkout is reference
material; it is not a runtime dependency.

| Change or boundary | Adaptation work | Owner |
| --- | --- | --- |
| egui/eframe, paint crates, kittest, standalone harnesses | Move the matched 0.37 family together, including explicit feature choices and all lockfiles | U15 official slice |
| `WidgetType` replacement and accessibility changes | Audit Role mappings, invisible/clipped controls, popup/modal trees and assertions | egui adapter |
| ID API changes | Audit deprecated `Id::new` and ID-salt APIs while retaining stable docking item identities | egui adapter |
| Font/provider and glyph-cache changes | Keep shaping in adapter; use explicit fonts for deterministic metric tests | egui measurement and harnesses |
| `Context::root_ui`, `run_pass`, repaint observer | Review run/pass entry points and wake handling; public additions do not by themselves prove native finalization | egui pass bridge |
| Screenshot callback, new event/shape variants, scroll source | Update actual matching call sites and inspection/harness code; preserve output order and blocker semantics | egui and smoke code |
| eframe raw motion routing and window restoration fixes | Re-evaluate custom patches against upstream behavior before carrying them forward | native adapter/fork |
| Final-pass hits and host finalization | Compare exact 0.37 source against each admitted seam; retain only missing required facts | U15 native slice |
| Winit desktop pointer and work-area facts | Keep the reviewed 0.30.13 fork until an independently justified migration | native host |

Apply the official dependency update before broad egui refactoring so migration errors remain separable from semantic changes.
The native fork rebase must precede its U7/U8 qualification, but does not block the independent core host work.
Replacement fork revisions are implementation outputs, not invented plan pins.
When changed, manifests, lockfiles, CI pins, seam admission documentation, and runnable examples move together.

### Alternatives Considered

| Approach | Benefit | Cost / decision |
| --- | --- | --- |
| Deepen the current semantic core and expose a complete host contract | Reuses validated topology, persistence and stale rejection; supports engine-controlled rendering | Chosen; requires deleting duplicated terminal owners and framework assumptions |
| Make the egui product facade the universal integration interface | Lowest immediate adapter churn | Rejected; imposes widget/pass assumptions on the engine and cannot prove host independence |
| Replace core wholesale with ImGui/Open-GPUI structures | Concentrated reference behavior | Rejected; imports framework lifetimes and discards useful tested invariants; algorithms may still be reused selectively |

### Reference Architecture Decisions

The updated ImGui reference is `repo-ref/imgui` docking commit `d88f238d1d363ef387382b2e29ad12cf96f826ac` from 2026-10-15 (`1.93.1 WIP`).
Use `ImGuiPlatformIO` and per-viewport draw-data separation to check the host/renderer split, and its centralized docking request/preview code to compare gesture locality.
Its queued frame timing, binary dock tree, global context, previous-frame target acceptance, and `p_open` lifetime are not normative core contracts.
Keep window DPI distinct from framebuffer density; core logical coordinates also support non-window engine surfaces.

### Risks and Mitigations

| Risk | Consequence | Mitigation |
| --- | --- | --- |
| A nominal host API still needs egui behavior | Engine integration fails after the adapter appears complete | U13 consumer uses public core only and exercises deferred records and two logical surfaces |
| Stronger names than the evidence actually supplied | Preview or receiver authority advances too early | R11 assigns emission and terminal reporting distinct guarantees without a GPU-proof subsystem |
| A generic program becomes a widget toolkit | Excessive public API and renderer coupling | R10/R22 restrict the program to docking composition and semantic content slots |
| Native lifecycle remains tied to eframe stages | Every new engine backend duplicates a long choreography | R15/KTD16 separate core admission from host staging strategy |
| Dependency migration hides behavior regressions | Failures become difficult to attribute | Separate U14, U15 official, and U15 native slices with existing gates |
| Portability work becomes another test framework | Maintenance grows without product evidence | One ordinary consumer, finite journeys, no generic selector or GUI automation DSL |

### Sequencing and Readiness Checkpoints

```mermaid
flowchart TB
  U14[U14 current correctness repairs] --> U13[U13 host contract entry slice]
  U14 --> U15A[U15 official egui 0.37]
  U13 --> U2[U2 emission and presentation closure]
  U3 --> U13B[U13 ordered logical-surface completion]
  U15A -->|egui slice| U2
  U2 --> U3[U3 one manipulation kernel]
  U3 --> U4[U4 magnetic and full content projection]
  U3 --> U5[U5 public host blocker port]
  U4 --> U6[U6 commands facade feedback]
  U5 --> U6
  U6 --> U12[U12 core readiness]
  U13B --> U12
  U15A --> U12
  U12 --> U10[U10 layout shell and showcase]
  U3 --> U7[U7 bounded ingress and handoff]
  U5 --> U7
  U15A --> U15B[U15 minimal native fork rebase]
  U15B --> U7
  U15B --> U8A[U8 resource ownership prerequisite]
  U8A --> U7
  U7 --> U8[U8 native runtime readiness]
  U8A --> U8
  U8 --> U11[U11 per-platform certification]
  U12 --> U9[U9 evidence and deletion closure]
  U10 --> U9
  U8 --> U9
```

U1's finite journey/oracle work continues as evidence for the behavior each replacement touches; existing valid cases are reused rather than restarted.
Core readiness (U12) means public engine integration plus egui local validation, without waiting for Open-GPUI or every native backend.
U5's common host port and truthful official-egui boundary satisfy U6/U12; exact fork-backed native hit parity is completed with U7/U8 after the native U15 slice.
Native readiness (U8, then U9) adds the optional core host lifecycle and qualified X11 reference.
U8's resource-ownership slice precedes U7's final capacity/overload integration;
its lifecycle and platform readiness verification still follows U7. Inspection
of the native callback path found that exact terminal admission otherwise needs
duplicate reservation maps for sidecars that U8 already requires joining.
This sequencing change preserves every U7/U8 exit criterion.
After the 2026-10-28 held-close ownership checkpoint, resume U3d's finite timed
continuation and U10's shared stationary overflow owner in core and official egui
before expanding the optional native overload/platform work. Metadata capture,
hover dwell and touch arming remain separate implementation/qualification steps;
the existing native resource checkpoint is retained. U3d now has a finite
core-owned stationary continuation, hover selection and explicit local-touch
source capture in official egui. Local active-drag edge scrolling shares that
continuation and the existing overflow owner. The following local-touch slice
binds expanded resize footprints to that same exact output; native stationary
capture still requires its own qualification. This priority change keeps
the custom-engine host central and does not remove U7/U8/U11 acceptance gates.
U11 gates Windows/macOS capability claims separately; unsupported backends remain explicit rather than blocking the generic crate.
These are packaging/readiness gates, not instructions to publish crates or upstream patches automatically.

---

## Implementation Units

### U14. Repair current configuration-dependent correctness defects

- **Goal:** Establish a sound baseline before changing dependencies or owners.
- **Requirements:** R11, R13, R15, R18; KTD6, KTD7.
- **Dependencies:** None.
- **Files:** `crates/egui_dockspace_native/src/coordinator.rs`, `crates/egui_dockspace_native/src/mailbox.rs`, `crates/egui_dockspace_native/src/host_frame.rs`, `crates/egui_dockspace_native/src/surface_driver.rs`, existing coordinator/mailbox tests, `.github/workflows/ci.yml`.
- **Approach:** Move required state updates out of debug assertions and ensure presentation commits drain in the same serial settlement boundary. Unify ordinary/staging/close submission exits where the existing presentation contract requires it.
- **Test scenarios:** With debug assertions disabled, create failure, pointer pass-through acknowledgement, terminal-roster rearm, and close settle correctly. An unchanged redraw followed by no input leaves the exact core/receiver artifact aligned and no pending ACK.
- **Verification:** Existing native tests pass in both assertion configurations; failed or abandoned host-frame retries do not duplicate output retirement.

### U15. Modernize the egui validation baseline and admitted fork seams

- **Goal:** Validate against the selected stable release without coupling core design to it.
- **Requirements:** R11, R12, R18, R23; KTD13, KTD15.
- **Dependencies:** U14 for the native regression baseline; official and native migration are separate reviewable slices.
- **Files:** `Cargo.toml`, all existing workspace/harness lockfiles and manifests, `crates/egui_dockspace/src/`, `integration/egui-official-harness/`, `integration/egui-product-harness/`, `integration/egui-fork-workspace/`, `integration/egui-native-smoke/`, `repo-ref/egui-release/`, `.github/workflows/ci.yml`, `docs/knowledge/egui-native-fork-seam-admission.md`.
- **Approach:** Apply the Dependency and Fork Adaptation table. Update official consumer/tests/examples first. Rebuild the minimal required native patch series on the same stable upstream before U7 qualification. Preserve unrelated dirty reference work.
- **Test scenarios:** Public consumer, font measurement, output-hook discard/unwind, IDs, accessibility clipping, popup/modal blockers, repaint, and native callback identity preserve their documented behavior across the update.
- **Verification:** Official 0.37 validation passes independently; the rebased fork has documented seam reasons and matching exact pins before native tests claim the new baseline. No UI dependency enters core.

### U13. Establish the public engine host contract

- **Goal:** Make self-drawn engine integration a first-class supported path.
- **Requirements:** R10-R12, R14, R16-R24; KTD1, KTD2, KTD6, KTD14.
- **Dependencies:** U14. Does not depend on the native fork rebase.
- **Files:** `crates/dockspace/src/runtime.rs`, `crates/dockspace/src/runtime/{session,host_frame,measurement,paint,presentation,interaction}.rs`, `crates/dockspace/README.md`, `integration/dockspace-host-harness/` (new), `Cargo.toml`, `.github/workflows/ci.yml`.
- **Approach:** Use two slices under this unit. The entry slice documents and exposes the necessary host facade, owned output records, renderer-neutral SourceGap obligations, and a tiny public consumer; it unblocks U2 without claiming complete pointer routing. The completion slice is implemented with U3's single manipulation owner and decouples ordered logical-surface input from native registration. One host-local producer retains one global event order; each edge carries independently qualified delivery/source and hover surface coordinates. Capture belongs to the host input domain, not the currently hovered logical surface. Surface removal or unavailable presentation invalidates the affected route rather than replacing the producer or resetting unrelated watermarks. Both slices are required for U13 completion.
- **Test scenarios:** Covers AE12-AE14. Compile owned records, release frame borrows, delay/drop/supersede outputs, reject stale receivers, and route two logical surfaces without OS-window enrollment. Verify default/serde dependency independence and layout/command use without pointer/native registration.
- **Verification:** The downstream crate uses no private modules, test-only authority constructors, or egui types; the documented host lifecycle is sufficient for its complete local journey.

### U1. Maintain the finite behavior spine and oracle classifications

- **Goal:** Preserve useful evidence while making engine-host and egui results comparable.
- **Requirements:** R1-R8, R17-R19, R24; KTD9.
- **Dependencies:** Reuse existing catalog immediately; add public-host execution after U13.
- **Files:** `crates/dockspace_host_conformance/src/`, `integration/dockspace-host-harness/`, `integration/egui-product-harness/tests/`, `docs/VALIDATION.md`.
- **Approach:** Keep L01-L08 and N01/P01 finite, add host scenarios AE12-AE14, and distinguish normative, characterization, and known-invalid observations. Replace failing behavior rather than relabeling it successful.
- **Test scenarios:** Center/four-edge semantic regions, complete same-tick gestures, hesitant releases, target loss, output drop, and local quiescence produce normalized observations without guide selectors or frame-count authority.
- **Verification:** Each claimed journey executes real public actions in its executor; catalog metadata alone is not a passing behavior test.

### U2. Close ordered emission and atomic current-output promotion

- **Goal:** Finish the existing program/presentation replacement for both engine and egui hosts.
- **Requirements:** R4, R5, R10-R12, R16, R21; KTD2, KTD6, KTD10.
- **Dependencies:** U13's entry slice and the relevant U1 journeys. Changes to the egui bridge additionally require U15's official slice; independent core work can proceed before it. U13's ordered-input completion is qualified with U3 and does not block this output-contract work.
- **Files:** `crates/dockspace/src/scene.rs`, `crates/dockspace/src/scene_compiler/`, `crates/dockspace/src/runtime/{paint,presentation,host_frame}.rs`, `crates/dockspace/src/{semantic_manifest,presentation_hit}.rs`, `crates/egui_dockspace/src/{output_proof,pass_settlement,product_dockspace}.rs`, public host harness and existing presentation tests.
- **Approach:** Retain U2a's canonical traversal and finish U2b's exact output commit path. Give execution receipts R11's limited meaning, remove public preview-only acknowledgement authority, and make receiver/semantic/program promotion one committed artifact. Treat mutable egui shape markers as adapter-local diagnostics unless their limited acceptance contract is explicitly proven.
- **Test scenarios:** Covers AE5/AE13. Complete/failed emission, dropped output, output-hook modification, unwind, late predecessor, failed core commit, and idle post-commit settlement never create mixed artifacts or release authority.
- **Verification:** One exact output path is used by public engine, egui, and native integrations; flat schedule reconstruction and preview-only acknowledgement bypasses are deleted.

Implementation evidence (2026-10-27): official egui now returns complete owned
output and an affine host disposition receipt, feeds exact core emissions, and
removes preview-only ACK and the old renderer-pass bridge. Conservative complete
span and pane callback checks qualify its single-layer contract. See
`docs/VALIDATION.md` for final gates, independent-review fixes and mutation-red
checks. This closes U2 output settlement; U3 retains the Burst and ordered-input
completion work.

### U3. Converge ordered manipulation and terminal state

- **Goal:** Make a gesture's complete lifecycle local to one deep core module.
- **Requirements:** R1-R4, R8, R9, R11, R14, R17; KTD3, KTD8, KTD10.
- **Dependencies:** U2.
- **Files:** `crates/dockspace/src/runtime/direct_manipulation.rs` or a focused module directory, `crates/dockspace/src/runtime/interaction.rs`, `crates/dockspace/src/{interaction,pointer_journal}.rs`, `crates/dockspace/src/engine/{host_frame,pointer_proof,provider_lifecycle,semantic_candidate,pointer_transaction,contained_adjustment,pointer_contained,pointer_splitter}.rs`, `crates/egui_dockspace/src/product_render/actions.rs`, public-host and product gesture tests.
- **Approach:** Normalize ordered input boundaries and semantic commands into one transaction owner. Replace the surface-fixed producer scope with a host-local input domain and event-bound qualified surface routes; retain the existing affine journal, exact receiver validation, drain and rollback. Source manipulation coordinates and target hover coordinates remain independent, including when two surfaces use identical local bounds. A departure from all logical surfaces grants no desktop outside-all fact. Keep semantic controls/scroll separate where their existing subsystem owns them. Remove old release/cancel/ACK terminal reducers and all inference of an event's surface from the producer lease rather than forwarding through them. A single-surface convenience API may wrap the same implementation without another terminal owner.
- **Test scenarios:** Covers AE2/AE4. Same-tick press/move/release, duplicate release, cancellation, capture/source/target loss, discarded preview, and policy changes produce one terminal outcome through both input adapters.
- **Verification:** Complete Burst cases enter ordinary passing gates; one core release barrier and cancellation owner remain.

Implementation is split at the host contract boundary:

1. **U3a — host input domain and shared terminal ownership.** Add one affine
   host-local producer, independently qualified manipulation/hover routes per
   edge, and public multi-surface batch submission without native enrollment.
   Reuse journal sequencing, receiver validation, frame rollback and drain.
   Merge tab/contained release consumption and owner cancellation across the
   existing entry points. Verify a complete public-host Burst, exact preview
   settlement, route isolation and retry before migrating the egui transport.
2. **U3b — ordered egui transport and legacy entry retirement.** Capture raw
   edges once per host/run boundary and qualify each chrome receiver at that
   edge's position. Submit to U3a's owner; UI reruns must not replay consumed
   edges. Remove pointer actions reconstructed from `Response`, the remaining
   local-response owner/frontends and internal preview ACK reducers. Migrate
   internal ACK fixtures to actual complete output observations. Keyboard,
   AccessKit and application commands retain their semantic entry points.
3. **U3c — full product matrix.** Run the original single-batch Burst without
   splitting it into input frames, and prove release/cancel/output/policy loss
   through both adapters. Retire the ignored gate only when this passes.
4. **U3d — timed input continuation.** Complete the already specified hover
   selection and local-touch profile through the same journal/gesture owner.
   Preserve device class and monotonic input time, expose only the finite
   gesture-bound wake/receiver challenge needed for stationary continuation,
   and reject stale output, owner, capture, policy and cancelled deadlines.
   Share that continuation boundary with U10 active-drag edge scrolling. A
   host schedules wakeups; it cannot mint pointer motion or mutate layout from
   a timer. Qualify 150 ms hover selection, 350 ms touch arming, 8-point slop,
   pre-arm pane scrolling, early activation and stationary context release.

The updated ImGui reference preserves event order by trickling queued input
across frames; this is evidence for retaining edges, not a same-frame Burst
implementation to copy. Official egui 0.37 exposes ordered raw events, but its
`Response` is a frame summary and cannot prove per-event widget ownership.
U3b must establish controlled chrome receiver facts; arbitrary application
widgets and blocker interoperability remain U5's explicit qualification work.
U3a alone does not complete U3 or the full U13/AE12-AE14 matrix.

Implementation evidence (2026-10-27): U3a now exposes the host-local public input
domain and shares tab/contained terminal consumption and exact-owner cancellation.
The public consumer executes complete ordered cross-surface releases, including
an unselected source, through actual owned recording-output settlement. Review
regressions cover source-only continuation after Raise, foreign pointer rejection
and presentation-stream takeover. See `docs/VALIDATION.md` for serialized final
gates and mutation-red evidence.

U3b transport progress (2026-10-28): the official egui adapter captures raw
events once at the owned run boundary and submits them before measurement and
paint. Reruns cannot replay successful or rejected packets. Click and drag
receiver lanes remain independent; exact output and completed-pass identities
bind the controlled chrome facts. Explicitly registered ordinary pane widgets
can pass the docking lane; unknown widgets and unsupported layer transforms
remain blocked. This does not certify arbitrary egui widget ancestry or popup
interoperability. Internal preview-only ACK reducers and renderer response-based
docking gesture producers are deleted. The original complete L03 product matrix,
including its unchanged single-batch Burst executor, now passes without ignore.

U3 replacement gate completed (2026-10-28): the core `LocalResponse` owner,
`SurfaceGesturePhase`, local gesture preparation/input APIs, local phase reducers,
local position identity, and gesture-specific `LocalReady` authority alternatives
are deleted. Every gesture now requires an admitted journal stream, a frozen
presented proof, capture authority and a threshold origin. Non-pointer candidate
commands remain in `semantic_candidate.rs` and `contained_adjustment.rs`; they do
not create another gesture owner. Useful presentation, contained and native
tests use actual host input and exact output settlement. U3c's unchanged complete
Burst matrix passes without ignore. Independent proof/continuation review and
fresh downstream gates are recorded in `docs/VALIDATION.md`.

The 2026-10-28 timing audit confirms U3a/U3b/U3c's migration and replacement
gates, but leaves U3d open. The follow-up input slice retains explicit device
class and optional monotonic time through core, local and native facade records.
The existing pointer ledger validates provider-wide time order and stable known
device class per live stream at its normal rollbackable commit boundary. Unknown
samples grant no device/time facts, overload keeps the provider clock, and only
a new provider may use a new origin. The official owned-run adapter forwards
explicit RawInput.time from the original ordered batch; reruns cannot replace it
with egui's predicted time. It retains Unknown device for generic pointer events
and does not claim per-OS-event timestamps.

The finite stationary slice now keeps one opaque continuation on the existing
active drag and returns it through an explicit non-motion journal edge. Its
150ms hover selection uses current framework/semantic receiver facts, retains
the dwell origin across equivalent emissions in the same stream, selects without
requesting content focus, and recaptures the same source for continued drag and
release. A valid source-only continuation may preserve an unselected source's
press activation; it never refreshes stale hover authority. Public-host and
official egui tests cover stationary execution, exact retry, no added focus and
final release. Motion only updates the challenge; timed selection uses an
explicit continuation after the host's accepted raw batch, preserving a release
which arrives at the deadline. Independent review found the source wake getter
too strict; its fix and a mutation-red inactive-source regression are recorded
in validation. A due Move+Release regression also guards terminal ordering.

The local-touch slice reuses the existing Pressed owner and click-bound finite
continuation. Explicit Touch contacts on manipulation chrome wait without Select,
Raise, focus or geometry changes. A current due continuation arms at 350ms;
pre-arm movement beyond 8 logical points yields to application content, early
release activates once, and stationary armed release emits a semantic context
menu request. Armed movement enters the same ordinary tab, splitter and contained
owners. Manipulation entry retains the displayed selection and contained order;
delivery performs the durable action. It does not revoke same-batch target
authority through an eager Select/Raise or broaden stale hover proof. Ordinary
close and strip/menu controls retain tap behavior with movement and release slop,
without a docking deadline.

The official egui adapter retains independent explicit-contact identities,
suppresses duplicate synthesized generic pointer ingress and reuses its existing
root presentation menu and prepared commands. Ordinary egui content input remains
owned by the pane. Public-host and product qualifications are recorded separately
in `docs/VALIDATION.md`; no physical touch-device claim follows from headless
journeys. Expanded local resize targets now share the exact output manifest,
device-aware query, receipt proof and gesture preparation. Explicit Touch uses
at least 44 logical points before clipping; all other device classes keep their
base geometry. Title/tab/close/menu chrome and visible front windows stay
protected; empty or ambiguous targets grant no input. A clipped-empty base
contained direction retains its semantic identity when a nonempty Touch fragment
is visible. Receiver descriptors continue to describe the base geometry. Native
metadata/stationary capture remains open. U10 local active-drag edge scrolling uses the
shared continuation owner; native stationary routing remains separate.
The full behavior plan cannot close before the remaining U3d qualification.

U3's replacement completion does not establish U4 full live-content projection, U5 broader
blocker qualification, U6's full command/focus contract, or U12 structural-cost
and engine-readiness certification. These remain separate gates.

### U4. Complete magnetic behavior and live contained content

- **Goal:** Deliver consistent direct manipulation through the host-neutral projection.
- **Requirements:** R2-R5, R8, R10, R17, R24; KTD2-KTD5.
- **Dependencies:** U3.
- **Files:** `crates/dockspace/src/drop_resolver.rs`, `crates/dockspace/src/scene_compiler/surface.rs`, `crates/dockspace/src/runtime/direct_manipulation.rs`, `crates/egui_dockspace/src/product_render/`, public-host and product projection tests.
- **Approach:** Apply transient placement before compiling contained chrome and pane subtrees. Preserve one target solver, source suppression, event-time dwell and frozen pending-release plan. Use cached valid measurements where possible.
- **Test scenarios:** Covers AE2/AE3. Non-guide basin samples, jitter, hysteresis-edge release, contained move/eight-way resize, target panes behind the source, and cancellation compare program, hit, semantic and durable results.
- **Verification:** Full pane content follows the projection; a window-sized empty ghost is insufficient. Structural cost meets R24.

Implementation boundary established by the post-U3 source review:

- Reuse `compile_contained_record`, `compile_root` and the existing surface
  compiler. Feed a core-owned effective outer rect before clipping, occlusion,
  chrome and subtree layout; do not scale cached pixels or add a second layout
  engine. Compiler and validator must consume the same authorized projection.
- Derive the projection from the active gesture or its frozen pending release.
  Only the existing terminal delivery path writes durable placement. A pending
  release keeps the complete content projection, not just a shell.
- Treat legal same-gesture projection updates separately from real scene,
  coordinate, policy or source loss. Changing the compiled scene must not cancel
  its own gesture or broaden old hover authority. Exact output settlement remains
  the release barrier.
- Separate source pixels from docking-target suppression: show the moved subtree
  once, but exclude the complete moving source from HoverDrop targeting and from
  occluding eligible targets behind it. Partial payloads, other contained roots,
  popups and unknown framework blockers do not gain blanket passthrough.
- Verify cached measurement coverage before introducing a reuse path; existing
  `PresentationLayoutFacts` alone have not yet been proven sufficient for every
  popup/measurement contribution. Do not invent a generic async layout cache.

The first implementation gates are multi-pane move with stable widget identity,
parameterized eight-way resize with subtree/hit/semantic agreement, an eligible
target behind the live source, pending Presented/Dropped settlement, and repeated
updates/cancellation without self-invalidation or redundant compilation.

### U5. Implement the host docking-hover port and egui blocker validation

- **Goal:** Authorize docking through ordinary content without crossing blockers.
- **Requirements:** R3, R12, R17-R19; KTD4, KTD13.
- **Dependencies:** U3 and U13; common egui coverage uses U15's official slice. The fork-backed native extension is qualified with U7/U8 after U15's native slice and does not block U6/U12.
- **Files:** `crates/dockspace/src/runtime/interaction.rs`, `crates/dockspace/src/runtime/native/pointer.rs`, `crates/egui_dockspace/src/product_render/`, `crates/egui_dockspace_native/src/receiver.rs`, the rebased egui hit-test seam, public-host and product blocker tests.
- **Approach:** Consume typed output-bound host answers through one port. Let the engine use its own composed UI facts; keep egui stack inspection in the adapter/fork and expose unsupported portable cases truthfully.
- **Test scenarios:** Covers AE1/AE1b. Actual TextEdit/ScrollArea/Button/canvas bounds, popup/modal/foreground overlay, explicit blocker, front contained root, unknown identity, and stale output prevent fallback through the front result.
- **Verification:** The public host consumes exact typed answers and official egui reports unsupported cases truthfully; no renderer reconstructs target policy from rectangles. U7/U8 additionally prove that the native path agrees on the same host-answer contract.

### U6. Finish core commands and the application-facing adapter loop

- **Goal:** Expose coherent product behavior without hiding required engine host APIs.
- **Requirements:** R7, R8, R16, R17, R19; KTD1, KTD11.
- **Dependencies:** U4 and U5.
- **Files:** `crates/dockspace/src/model/`, `crates/dockspace/src/runtime/{close,report,session}.rs`, `crates/egui_dockspace/src/{builder,product_dockspace,product_response}.rs`, `crates/egui_dockspace/src/product_render/`, existing examples, public-host and close/feedback tests.
- **Approach:** Keep R7 common-path stable-item commands and typed manipulation feedback in core; R25 layout-management commands stay in U10. Fix Close Group versus Dock Back, consume close decisions in examples, and map semantic focus intent to adapter behavior. One-call convenience remains an adapter concern.
- **Test scenarios:** Covers AE8 and the common-path portions of AE9/AE10; their R25 locked/reopen/restore cases remain in U10. Immediate/deferred close resolve exactly once, examples actually complete permitted closes, rejected/pending results deduplicate, and focus returns to the documented successor.
- **Verification:** Both public engine integration and the minimal egui example can discover and complete the common local workflow without raw graph or proof internals.

### U12. Certify core host readiness

- **Goal:** Make the reusable crate independently consumable before expanding native platforms.
- **Requirements:** R1-R5, R7-R12, R16-R24; KD7; KTD14.
- **Dependencies:** U6, U13, and U15's official slice.
- **Files:** `crates/dockspace/README.md`, public host harness, official/product harnesses, package manifests, `README.md`, `docs/VALIDATION.md`, `.github/workflows/ci.yml`.
- **Approach:** Package the core host contract, dependency independence, and complete local/contained journeys. Record adapter capability limits separately from core guarantees and give an engine author a complete integration example.
- **Test scenarios:** A clean downstream dependency resolves the published-shaped core API; the same normal journeys run through immediate/deferred public hosts and egui; no ignored Burst or old ACK remains a claimed passing gate.
- **Verification:** Core tarball, strict docs, dependency tree, public consumer, and common-path validation pass without Open-GPUI or platform-specific native certification.

### U7. Replace native callback throughput and add scoped input continuity

- **Goal:** Make optional native hosting bounded and preserve useful local behavior without desktop facts.
- **Requirements:** R6, R9, R11-R14, R18, R20; KTD7, KTD8, KTD12.
- **Dependencies:** U3, U5, U14, U15's native slice, and U8's resource-ownership prerequisite for final bounded terminal admission.
- **Files:** `crates/egui_dockspace_native/src/{mailbox,coordinator,pointer_event,input_control,surface_driver}.rs`, core ingress/pointer modules, native coordinator tests, public-host scoped-input tests.
- **Approach:** Replace one-callback-per-cycle front/ACK families with bounded immutable prefix claims. Establish surface-local routing and validated desktop handoff through the same core kernel; preserve exact output and lifecycle barriers.
- **Capacity integration:** Keep capture identity and original age through raw-to-core handoff until the committed boundary releases its source reservation. Share immutable recorded payloads across retry batches. On overload, freeze ordinary admission at one fixed capture boundary B; commit all currently replayable accepted input through B before an independent terminal cancellation frame. Accepted lifecycle records parked behind unresolved external dependencies retain their existing owner, capture identity, age and reservation across cancellation; they publish only after their original dependency and commit barriers settle. Post-boundary callbacks that settle existing affine output/create/command/close/window obligations use already reserved, once-only owner slots with original capture order; they do not move B or create a second unbounded journal. Neither an unresolved held-close successor nor an unrelated post-B terminal delays live-input cancellation; source reservations already transferred to core still require their complete original boundary commit. A new ordinary pointer callback rejected after freeze is not an accepted edge; the platform still processes it, and overload cancellation ends the surviving core input. Only successful cancellation commit resets translator state, preserving identity counters and without emitting synthetic releases or claiming an empty physical roster. Pending releases retain their separate exact-output obligations.
- **Storage implementation direction:** Reuse the serialized callback boundary for a bounded raw backing and one shared core/replay work allowance, instead of reserving a complete core payload copy for every queued source or introducing per-allocation authority tokens. Reclaim committed recorder payloads before the next raw transfer, or charge their actual overlap. Effect/presentation preludes and whole atomic terminal groups use the same allowance; the 256-record scheduling limit is not an atomic-group storage bound. Prospective complete-roster size must be checked from borrowed facts before capture, and existing terminal owners must reserve variable backing when their obligation is admitted. The final implementation must state and enforce the maximum complete roster and terminal fanout it can retain, without truncating Exact facts or dropping accepted terminals. Shared roster data is counted by allocation identity through its existing owners, without a persistent address registry.
- **Core capacity boundary:** Keep generic/core host recording unbounded unless its host selects a limit. The optional native profile can preallocate a finite recorder buffer and reserve batch, guard and retirement-receipt transport space for its complete admitted record set, including whole atomic groups. Final admission belongs at the recorder's single append boundary; finite snapshot/checkpoint construction also needs prospective checks. Capacity rejection preserves the existing producer/output owner and allows already accepted input to replay instead of trapping poll in a full-prelude error loop. Enrollment must reject insufficient storage before publishing an ownerless provider, without panic. Resource/adapter data and shared core allowance must be combined before claiming the total 4 MiB gate.
- **Prepared pointer transfer:** The public batch/sequence path uses bounded prepared journals, reserves their handle buffer, and charges journal contents once as they move into the recorder. Prospective final append may subtract only the safely reclaimable committed prefix; the staging peak retains all existing payloads, including committed data. A suspended replay receives no retirement credit. Recording stops at the first nonadmissible item and completes caller iteration and destruction before maintenance or append. Iterator panic, capacity rejection, and invalid later facts preserve the published workspace and producer watermarks. This closes the iterator-collection prerequisite, not the combined 4 MiB gate.
- **Binding terminal fact:** Destroyed-window input termination records one fixed-size exact-binding backend fact. Core derives affected held-button/capture streams and closes their scroll provider tails without adapter-generated pointer fanout. The original physical-window receipt retains once-only recording progress across later lifecycle failures and retirement moves; independent destruction/output/quiescence barriers remain authoritative. Do not reserve a synthetic pointer-record group per active device for this platform fact.
- **Test scenarios:** Covers AE7/AE11/AE14. Retry after a rejected batch, 10,000 interleaved records, sustained 1,000 Hz drag, overload, local touch coordinates, and scope/provider changes preserve one terminal release.
- **Verification:** Resource budgets hold and no unbounded journal or forced desktop-only local route remains.

Local fixed-boundary checkpoint (2026-10-29): ordinary source admission freezes
at 4,096 retained records or 250ms oldest age, retaining the first B. Existing
core boundaries and currently replayable input through B commit before one
exact overload cancellation; parked lifecycle dependencies and post-B terminals
retain their owner without delaying cancellation. Only its matching successful
commit resets translator state. Control/prelude frames remain inert, and the
terminal roster has one inline slot gated by full commit plus exact snapshot
settlement. Recovering and fatal modes are independent. The 4 MiB owned-capacity
measurement/preallocation, native scoped input, load/release latency and soak
remain unfinished; this checkpoint does not qualify the complete U7 budget.

Local capacity-measurement prerequisite (2026-10-29): the public session and
open-frame queries count recorded native transport envelopes, payload/vector
capacity, shared retry data once, replay buffers, and retirement receipt
storage. Commit and retirement do not falsely release backing capacity.
Unsupported semantic shapes fail explicitly instead of returning a partial
count. Host/pending-report buffers, reducer/candidate state and allocation
overhead are outside this narrow query. Actual 4 MiB ordinary admission,
raw-to-core peak reservations and protected variable-payload preallocation
remain unfinished.

The optional recorded-transport primitive now admits a host-selected record and
byte limit while the generic core/default facade remain unlimited. Its fixed
allowance includes actual recorder backing, a complete N-record replay batch,
the overlapping N-record commit guard, and the N-binding retirement receipt.
Payload capacity is charged once through the recorder; rollback and retirement
release only the corresponding retained charge. Enrollment initializes storage
and the first checkpoint before publishing either provider. Complete snapshot
and pointer-roster payload construction has a prospective check and explicit
vector capacities; every final append rechecks actual capacity before minting.
Full abandoned-effect or presentation preludes retain their existing owners and
allow the accepted prefix to commit. Matching partial presentation reports keep
their exact settlement set, and blocked or newly abandoned reports preserve the
input-pending barrier across suspended replay, dropped candidates, and commit.
The subsequent prepared-pointer transfer includes its journal handle buffer in
the fixed reservation and checks contents at both staging and final-append
peaks. The iterator's size hint cannot control allocation, and its construction,
iteration and destruction finish before maintenance. Producer maps and
adapter/resource owners still require the combined 4 MiB admission gate.
This checkpoint does not qualify U7.
Its prepared-transfer source passes 1,777 all-feature workspace tests, 80 public-host tests,
ten doctests, strict rustdoc and correctness/suspicious Clippy. Native tests pass
316 normally and 316 without native debug assertions; the local unpublished-fork
source also completes the macOS programmatic redock journey. These checks do not
certify physical input, mixed-DPI behavior or remotely reproducible fork pins.
A disposable-snapshot negative control prematurely credits committed data during
staging and fails the peak regression; restoring the implementation passes it.

Native owned-data integration now measures the existing host owners together:
raw and two shared roster backings; exact route/index, window/output/close and
retirement entries; snapshot/work-area sidecars; effect/input controls; pointer
devices and retained tombstones; core recorded transport and pending report
queues. Independent same-window entries remain charged, while roster and
abandoned-result backing is counted once through existing ownership. Boxed close
decisions and spare vector capacity are included. Native effect command storage
is restricted to its actual finite Close/Focus producers. The real-window smoke
samples the public app query at root boundaries and records a high-water mark.
This closes the current-data measurement gap, not prospective admission: shared
host/core allowance, original-owner future terminal reservations, constructor
peaks, scoped input and load/soak qualification remain required.
The source passes 1,778 all-feature workspace tests, 80 public-host tests, strict
docs/doctests and correctness/suspicious Clippy. Native tests pass 328 normally
and 328 without native debug assertions; macOS programmatic redock completes
with a sampled root-boundary high-water mark of 2,024,872 bytes. A targeted
duplicate-roster-charge control fails, and restoration passes. The complete 4 MiB
admission gate remains open despite this sampled workload fitting its target.

The native core-capacity slice now clips cursor claims to the facade's explicit
preparation allowance and streams translation into prepared journals. Temporary
full storage preserves the raw owner and translator candidate while the existing
accepted core prefix commits; empty-core permanent insufficiency remains an
explicit error. Effect results reclaim committed storage before retry, preserving
their affine result on rejection and their order ahead of subsequent pointer
recording. The bounded native qualification path is selected internally; default
product recording remains unlimited until future terminal fanout/storage is
reserved before obligation admission. This is not the final 4 MiB product gate.
The slice passes 1,780 all-feature workspace tests, 80 external public-host tests,
ten doctests, strict docs and correctness/suspicious Clippy. Local native gates
pass 330 tests normally and 330 without native debug assertions; the macOS
programmatic redock journey completes with a sampled root-boundary high-water
mark of 2,024,952 bytes. Physical input and mixed-DPI qualification remain open.
A disposable-snapshot control removes only cursor-prefix capacity clipping and
fails the bounded transfer regression; restoring clipping passes that test.

The subsequent binding-input slice removes Destroyed cancellation/reset vectors
and their separate pointer-prefix boundaries. One exact-binding backend fact
uses retained held-button origins, capture dependency across Unknown, and initial
scroll delivery to terminate only matching live input and provider tails. A
physical-window receipt retains once-only recording progress through later
failure and retirement migration. Other windows, pending release/output owners,
pointer sequence and lifecycle/quiescence barriers survive. One-slot/five-stream
core/public regressions, exact-scope/ABA cases and the existing real presented
receiver and pending-release matrix exercise this path. Product recording stays
unlimited until the shared host/core allowance, remaining terminal promises and
constructor peaks are enforced and qualified.
Fresh gates pass 1,786 all-feature workspace tests, 81 public-host tests, ten
doctests, strict docs and correctness/suspicious Clippy. Local native gates pass
332 tests normally and 332 without native debug assertions. The macOS
programmatic redock journey completes with a sampled root-boundary high-water
mark of 2,024,888 bytes. These results do not qualify the total allowance or
physical input.
A disposable-snapshot control omits only binding-fact staged-ledger sync and
fails the real presented-receiver same-batch regression with
`ButtonAlreadyPressed`; restoring synchronization passes the same regression.

The close-decision follow-up removes full plan copies from the native close
owner and its driver. The existing owner retains only `CloseRequestId`; the
public host-frame iterator reads current pending initial tokens without a full
plan clone. Driver request/item preparation and each decision share the current
record/deadline budget. Partial commits retain the exact request and existing
root wake; only a successfully committed, complete request releases its metadata.
Core application reports keep their complete-plan contract. Public bounded
300-pane and drop/retry coverage, native prefix gating and deferred/foreign/veto
cases qualify this contract independently of renderer/platform behavior.
This reduces promised terminal storage but leaves the combined allowance,
output/report obligations, constructor peaks and complete U7 gates open.
Fresh gates pass 1,786 all-feature workspace tests, 84 public-host tests, ten
doctests, strict docs and correctness/suspicious Clippy. Local native tests pass
337 normally and 337 without native debug assertions; the macOS programmatic
redock journey completes with a sampled root-boundary high-water mark of
2,024,968 bytes. This is local unpublished-fork qualification, not physical input
or a complete-budget claim.
A disposable-snapshot control removes only per-decision record charging and
fails the 259-pane/three-record regression; restoring charging passes it.

Final admission work is limited to four existing growth boundaries before the
product profile can be enabled:

1. Output-token owners must reserve ordinary capacity before insertion while a
   bounded root diagnostic/control path still drains accepted input and closes
   its exact token. Rejection cannot merely omit an owner and reinterpret the
   later renderer terminal as a protocol error or require a rejected-token set.
2. Pointer translation must reserve overlapping candidate data and every new
   device/button/tombstone entry before allocation. Moving a device into a
   tombstone does not release its retained-data charge.
3. Presentation/effect obligations must reserve future report/drop capacity at
   emission, so a terminal callback or affine `Drop` never competes with
   ordinary admission for unpromised space.
4. Roster conversion, producer generation maps and work-area/acknowledgement
   sidecars must reserve their construction and replacement overlap before
   materialization, including the still-live old backing.

Use one fixed core/replay allowance plus the remaining host allowance under the
existing serialized bridge ownership. Current owner data, promised terminal
space and construction overlap must fit together; root-boundary sampling is
only a cross-check. Keep the generic core unlimited by default. Do not add a
per-allocation registry, second lifecycle authority or another measurement-only
prerequisite in place of closing these four admission sites. The exact profile
sizes remain implementation choices subject to complete-budget qualification.

The output-admission implementation now checks ordinary output count (256) and
the selected private combined host allowance before entry insertion. One inline
root owner and a preallocated 256-child control/staging backing remain charged
independently of occupancy. Each child physical window can occupy one protected
slot; existing staging retains its original affine terminal until full commit.
Frozen ordinary admission still permits prepaid control progress. Root control
identity forbids new semantic paint/application actions even after cancellation
changes the mode. A new fork `Proceed | Defer` result prevents a busy-slot
candidate from running UI, consuming buffered egui input, entering the output
stack or creating an unowned terminal; both renderer paths ask before input
extraction. Rejection uses the existing durable root wake and no rejected-token
registry. This closes output entry insertion, not the remaining three growth
boundaries or the complete byte profile. Physical text/key retry and configured
product-cap saturation remain later qualification requirements.
The local fork commit is `af92097942f551914644f9d36f73dfba948e4997`, unpublished
with tracked pins unchanged. Native gates pass 339 tests normally and 339
without native debug assertions, plus correctness/suspicious Clippy. Fork Glow,
WGPU and combined-renderer no-assertion gates pass 81, 75 and 81 library tests;
both renderers compile without the seam. Programmatic macOS redock completes
with a sampled root-boundary high-water mark of 2,384,816 bytes, including the
prepaid control backing. Omitting only external-work reservation charging fails
its regression and restoring it passes. These checks do not replace configured
cap saturation, physical input or the remaining combined-budget work.

The pointer-translation slice now promises each accepted raw pointer's maximum
device/button growth before minting its source reservation. Existing translator
data and its overlapping candidate share the same private host allowance. Core
retention, candidate installation, destruction of the old backing and raw-source
transfer precede returning excess growth capacity, under the existing bridge
lock. A rejected recording leaves the original translator and exact source
available for retry. Source count and age still survive until full core commit.
Device-removal and phaseful-scroll tombstones reuse the original device entry;
active/cancelled buttons and their origins share one button entry. Binding,
provider and overload terminals therefore need no new translator allocation,
and retained tombstones keep their full entry-data charge. New pointer admission
checks its growth together with one ordinary output entry, so consuming the last
source allowance cannot strand a retained scroll tail in Recovering.
This closes the second of the four growth boundaries. Future presentation/effect
report promises and roster/producer construction peaks still block product
profile enablement and the complete 4 MiB qualification.
Fresh native gates pass 344 tests normally and 344 without native debug
assertions, plus correctness/suspicious Clippy and the programmatic macOS redock
journey. A negative control removing only output headroom fails its regression;
the exact restored source passes. Validation records the source-saturation,
retained-scroll recovery and real coordinator/core drop/retry coverage.

The remaining admission work uses the final prepared core transition before
publication as the report-prepayment boundary, including document restore.
Presentation terminal storage belongs to the existing pending output; effect
storage must survive request/result/acknowledgement handoff through the existing
owners. Roster conversion must cover the complete live-plus-destroyed merge
(conservatively up to twice the window bound), old/new work-area backing and
producer-generation candidates. Core producer limits need their own selected
finite profile: later adapter route admission cannot retroactively bound an
already published core registration. These are implementation requirements,
not new allocator registries or another lifecycle authority.

The core report-prepayment implementation now prepares the final transition
before publication, including document restore. Presentation output/result
storage is one shared pending slot; Drop fills that slot directly. Retained
generation/recorded metadata and submitted observations use flat backing with
explicit capacities. The selected `NativeReportStorageLimit` separately bounds
unsettled outputs, retained stream history and externally delivered effect
failure-report capabilities. Enrollment validates current replacement overlap
and preallocates before publishing a provider; the generic/default path remains
unlimited.

Effect requests prepay their complete future abandoned-result capacity, transfer
that reservation into explicit negative results and cleanup correlation, and
release it only at successful recording, intentional result disposal or accepted
dispatch. Shared queue draining and adapter retry preserve their existing backing.
Accepted-dispatch acknowledgements keep their existing identity and lifetime
owners; the remaining owner/lifetime growth boundary must combine their storage
with the selected report and transport allowances.

Finite report backpressure cannot roll back an accepted native prefix forever.
Core takes only the delivery prefix that has report storage, leaving its suffix
Requested and undelivered on the original effect ledger. An accepted prefix can
commit first so an older failed/abandoned result can enter freed ingress; the
existing effect is then delivered without a new identity. New application-only
operations which require unavailable effect delivery still reject atomically.
After all inputs, queued native close resolutions are reconciled against their
exact current plan: cancellation invalidates an undelivered Accept, while an
already delivered request keeps its causal predecessor role. Targeted real
staging-show/close and deferred-root-Accept cancellation regressions exercise the
full-prefix rollback/retry, original resources and once-only delivery. Complete
fresh qualification is recorded separately in validation.
The frozen source passes 1,801 all-feature workspace tests, 85 public-host tests,
ten doctests, strict docs and correctness/suspicious Clippy. Five report/progress
regressions also pass without core debug assertions. Native gates pass 344
normally and 344 without native debug assertions, plus Clippy and macOS
programmatic redock. These qualify report storage/progress; producer/lifetime
and constructor overlap, combined-profile load/soak and native scoped input
remain open.
Negative controls independently omit delivery backpressure and obsolete-close
invalidation; each fails its regression and exact restoration passes the three
progress/cancellation cases. No physical or complete-budget claim follows.

The producer/construction slice adds a separately selected
`NativeProducerStorageLimit`. Current and retired exact lifetimes share flat
binding storage until successful quiescence append; current admission and window/
close generations remain on that same entry. Enrollment validates the initial
registry and the complete old/new producer, returned binding/admission vectors,
snapshot facts/generation scratch and payload-construction peak before provider
publication. Every final native host-frame candidate, including layout reset and
document restore, prepares producer replacement and its report before core
publication. Snapshot iteration ignores size hints and installs generation
candidates only after successful recorder append. Snapshot constructors use
in-place sorting and validation without temporary tree allocations.

Registration preflights current, retired and queued lifetime demand before append.
A candidate-local allocation allowance also covers accepted pointer releases,
product tear-off and automatic replacement. Full lifetime storage returns a typed
rejection or host-recovery boundary instead of trapping an accepted prefix in
permanent commit rollback. A release waiting for exact preview presentation has
the same terminal capacity rejection, using its original cause and manipulation
identity; repeated dropped candidates do not duplicate its presentation fact or
terminal feedback.

The native adapter selects the producer's window/display bounds for a separate
fixed snapshot promise under the same private host allowance. Borrowed capture
checks logical bounds before minting a source. Conversion reserves the complete
live-plus-destroyed merge and ACK/focus sidecars; prepared work-area storage owns
the future committed routes, so successful core publication is followed only by
filling and moving existing backing. Capacity rejection preserves the original
roster envelope. Oversized display input revokes only work-area authority before
validation; borrowed and frozen-source deduplication preserve this normalization
and its capacity diagnostic. Generic/default producer storage remains unlimited.

This closes roster/producer construction admission as a selected primitive.
Host lifecycle/command owners still need a common prepaid allowance, including
multiple retained resource entries for one exact lifetime, successful-dispatch
ACK ownership, retirement and deferred/control construction. Those owners must
be bounded before enabling the combined 4 MiB product profile. Configured-cap
load/latency, lifecycle soak, native scoped input and physical platform gates
remain open; neither default-path sampling nor this primitive completes U7/U8.

The next owner-admission slice must use an explicit resource-entry bound for
`(ViewportId, NativeSurfaceBinding)`, rather than assume one entry per window
lifetime. Provider/viewport handoff may retain old awaiting or captured command
owners alongside a successor. New command admission must precede backend queueing;
staging must reserve its whole owner set before core commit. For input control,
the existing effect lease bound E proves at most E held requests/results, but one
accepted queue head can additionally await snapshot after releasing its lease:
pending backing therefore needs E+1 entries. Stable input observations can use
the producer lifetime bound W only if manual quiescence shares the automatic
path's reference guard. Retiring input should rotate the original deque and
move results directly into the prepaid result owner without a replacement deque
or temporary result vector. These are bounded implementation tasks on existing
owners, not another lifecycle registry or a presumed constant fanout multiplier.

Fresh producer/construction qualification passes 1,813 all-feature workspace
tests, 86 public-host tests, ten doctests, strict rustdoc and correctness/
suspicious Clippy. Native gates pass 360 tests normally and 360 without native
debug assertions, plus Clippy and macOS programmatic redock. The latter samples
2,385,753 owned input-data bytes at root boundaries without enabling the product
profile. A control removing only the pending native-release capacity terminal
fails its real gesture regression; exact restoration passes all ten producer
regressions normally and without core debug assertions. Final accounting adds
the empty retirement-receipt Option to transport's resident and prepaid storage;
the matching 27 storage/producer/byte-boundary cases and full workspace pass.
These gates qualify this slice only; the remaining owner-admission and platform
requirements above still apply.

The resource/input/effect-owner slice implements independent M exact resource
entries and C ordinary command entries. Resource keys retain viewport plus exact
binding, so old provider/viewport command owners coexist with successors without
being merged by window lifetime. Finite backing is preallocated and remains
charged after prune/drain. Create/show effect stores share the original E request
lease bound; stable input observations use W exact binding entries and pending
input prepays E+1 for the accepted head which still awaits snapshot. Input
retirement rotates its original deque and transfers affine results directly into
the existing prepaid result owner.

The local eframe seam prepares an affine property command and its exact token
without enqueueing backend work. Adapter focus, visibility and pass-through
dispatch reserve their terminal receipt and retain their original effect owner
before enqueueing. Capacity refusal produces a typed predispatch failure. Native
staging prepares all entry/request/wake backing under the existing host lock,
holds that lock across core publication, then fills existing slots; no callback
can consume the prepared space in between. The public core staging API returns
a borrowed exact-size iterator instead of silently collecting a temporary Vec.
The necessary deferred-viewport declaration copy is explicitly reserved before
releasing the runtime lock to call egui.

Manual binding quiescence now shares the automatic host-reference guard,
including stable input observations, exact routes, unfinished physical receipts,
callbacks and sidecars. Missing pending work alone cannot release W. The selected
private owner allowance combines current resource backing, staging scratch and
external input/effect/declaration promises with the same bridge allowance. This
does not yet prepay retirement/close-control storage, route/index and semantic
wake sets, or coordinator presentation/no-frame ACK storage and retirement
construction. Those remaining existing owners must close before combined-profile
enablement; configured-cap load/soak, scoped input and physical gates remain open.

Fresh owner-slice qualification passes 1,813 all-feature workspace tests, 86
public-host tests, ten doctests, strict docs and correctness/suspicious Clippy.
Native qualification passes 387 tests normally and 387 without native debug
assertions, plus Clippy and macOS programmatic real-window redock. A negative
control omitting the extra accepted-head slot fails, and exact restoration
passes all 18 targeted owner cases. The local unpublished fork commit
`bac446e142f93a5dc92695ab32e15a12e6dc7453` passes Glow 82, WGPU 76 and
combined no-assertion 82 tests, plus both renderers without the seam. These
results qualify this owner slice; they do not enable the complete product cap.

The next retirement/close slice should consolidate materialized and failed-create
retirement entries into one lifetime owner with a shared W backing. Release must
reject an alias collision before moving a physical receipt or acknowledgement.
Close capture retains its exact binding, current request and one parked successor;
it needs an independent entry allowance rather than an assumed W multiplier.
Its entry must exist before Hold, while cancellation, Destroyed and cleanup
callbacks only fill the original prepaid fields. Retirement route preparation,
quiescence and close-policy iteration should eliminate avoidable collected
vectors and prepay any necessary affine preparation before irreversible moves.
Subsequent route, semantic-wake and ACK owners require their own admission:
current routes are a W subset, but retained exact-binding presentation and
no-frame obligations cannot inherit W or the released effect lease bound E.

The local lifecycle-owner implementation now uses one sorted retirement Vec
for Window and Unmaterialized receipt kinds, with exact-binding lookup and
lifetime exclusion across both kinds. Route/create admission prepares the
future terminal backing before accepting the obligation. Refused release and
failed-create transfers return their original capabilities. The selected finite
close store prepays an independent L exact-entry bound and keeps each current
capture, cancellation receipt and parked successor inline; full storage still
admits existing-owner terminals and exact duplicates.

Current viewport routes use one W-sized Vec, with window publication,
pointer suppression and focus reportability on the original route. Surface
and window reverse maps and the focus set are deleted. A W-sized exact marker
Vec preserves retirement guards even after a successor occupies the viewport.
Whole-batch validation and backing preparation precede marker installation.
The retirement guard owns a single routes Vec plus an output-owner-sized token
sink; commit moves both buffers instead of cloning routes or collecting one
temporary token Vec per binding. Quiescence visits exact candidates individually
and retains blocked or append-rejected owners. Close policy selects requests and
decision tokens individually, compresses completed IDs into its one prepaid L
buffer, and settles those IDs only after frame commit. Fresh native gates pass
410 tests normally and 410 without native debug assertions, correctness/suspicious
Clippy and the macOS programmatic real-window redock. Independent retirement,
route and close-driver reviews report no remaining concrete blocker in this
slice. Semantic wakes, presentation/no-frame ACK ownership and remaining output/
construction backing still precede combined profile enablement; the complete
4 MiB, configured-cap load/latency, soak and physical gates remain open.

The subsequent observation slice moves presentation ACK and NoFrame state into
the original M exact resource entries and deletes both coordinator maps. Route
admission prepares that entry before mutation; its backing pin is refreshed from
existing routes and announced outputs. Exact observation/callback obligations
survive retirement and settlement, while a pin alone remains idle. NoFrame
abandonment and retention are atomic under the host lock; readiness and surface
selection reuse one prepaid ready Vec and one repaint Vec. Roster compilation
borrows ACK lookup under the destroyed-observation lock without another copy.

Semantic wake state is inline on each viewport route, replacing both sets and
the production ready Vec. Same-viewport replacement preserves its wake, exact
removal ends it, full clear boundaries release it, and repaint calls occur after
claiming one route outside the lock. Ordinary output's existing O=256 bound now
has fallibly prepaid sorted backing; root and child control stores retain their
independent capacity. Shared admission charges this backing in the baseline
instead of promising another per-entry output increment.

Fresh gates pass 427 native tests normally and 427 without native debug
assertions, correctness/suspicious Clippy and macOS programmatic real-window
redock. A pin-removal negative control fails the finite NoFrame regression;
exact restoration passes all 19 selected observation/wake/owner cases. Independent
wake/output/observation reviews report no remaining concrete blocker in this
slice. Output-prefix, unfinalized and quarantine construction, complete initial
combined admission and full-O/configured-cap load still precede product-profile
enablement. The complete 4 MiB, latency, soak, scoped input and physical gates
remain open; ordinary prepayment does not by itself qualify saturation.

The output-construction follow-up replaces output-prefix and unfinalized-output
vectors with one frozen journal-ordinal range. Next copies an entry from its
original output owner's terminal ingress, advances before callers remove that
owner, and never reapplies Replay or the submitted-prefix skip. Missing members
remain a protocol error. Quarantine claims one original output at a time, drops
paint and notifies pass-action cleanup outside the lock, then cancels unstarted
effects one at a time. Pass-action cleanup selects the two existing stores'
minimum key instead of collecting a token set or intermediate error vector.

Independent review found two additional shutdown boundaries: a create with a
physical capture still retains its original request before Created reduction,
and an unattached unsubmitted NotPresented terminal behind an earlier callback
must be abandoned during fatal drain. The existing route/receipt and output
owners now supply both checks. A core-effect owner regression covers admitted
and captured create models through repeated quarantine; no fork output or create
token is fabricated. Real Created terminal ordering and full-output saturation
remain explicitly separate from this model's strength. Fresh qualification passes
428 native tests normally and 428 without native debug assertions, correctness/
suspicious Clippy and the macOS programmatic real-window redock. Final independent
review closes both findings. Complete initial combined admission, full-output
saturation, actual batch/release timing and the remaining configured-cap/soak
and platform gates still precede product-profile enablement.
A negative control restoring the awaiting-attempt cancellation check fails the
captured-create regression; exact restoration passes the four selected create/
replacement/shutdown cases. Initial-constructor primitives are prepared in a
separate unqualified slice and are excluded from this tested source state.

The following initial-storage slice replaces raw journal and both roster
payloads' infallible vector allocation with checked fallible preparation and
actual-capacity validation. A fresh bridge reservation query accounts for all
initial backing. The private combined profile checks fresh bridge, pointer retry,
snapshot, complete owner and core I/P/R allowances plus uncovered coordinator
inline before allocating owners or publishing a provider. The existing bridge
allowance carries all selected promises afterwards without recharging complete
O, P or R resident state. Snapshot/work-area preparation now also precedes core
enable. A controlled-clock 4 MiB fixture proves complete initial difference-of-one
rejection, exact enrollment and pointer-growth refusal before capture identity.

Fresh gates pass 439 native tests normally and 439 without native debug
assertions, correctness/suspicious Clippy and the default-path macOS programmatic
redock. A negative control removing pending pointer growth fails the finite
regression; exact restoration passes 15 selected initial/owner tests. Two
independent constructor reviews report no remaining concrete blocker. The app
does not yet select the combined profile: real finite execution, saturation
recovery, full-output pressure, 1,000 Hz/release timing and 10,000-cycle soak
remain its enablement gates. No full U7/U8 or physical readiness follows.

The bounded-host qualification slice exposes `NativeStorageLimit` as an explicit
application choice, reusing the same combined constructor rather than changing
generic/default hosting. The app can report selected reserved bytes separately
from resident capacity. The smoke accepts bounded or unlimited storage through
one environment selector; bounded root boundaries require resident <= reserved
<= 4 MiB, and its output labels the selected profile. A preliminary bounded
macOS redock completes first-live and retirement under that same limit.

The saturation regression now drains the exact accepted prefix, retries a
dropped prefix frame, drops/retries cancellation, requires one overload terminal
and verifies fresh input can enter without raising the allowance. Coalescing and
retained core pointer reduction share one budget, after committing the freeze
prelude before raw replay. A separate 10,000-lifetime model reuses one coordinator,
resolves rebased surfaces through a stable application item and checks exact
Destroyed/tombstone/route/quiescence ownership and plateaued idle capacity.
Periodic candidate/preparation drops and late predecessor callbacks exercise
retry and incarnation isolation. Fresh qualification passes 442 complete normal
native workspace tests plus one public-only profile test, 443 complete tests
with native debug assertions disabled, correctness/suspicious Clippy and strict
rustdoc. Both complete runs execute the 10,000-lifetime model. The bounded
macOS redock completes first-live and retirement with sampled resident peak
3,005,859 bytes and reservation peak 4,122,680 bytes under the selected
4,194,304-byte limit; the explicitly unlimited comparison samples 2,750,705
resident bytes. This qualifies explicit bounded application construction,
finite real-window redock, byte-saturation recovery and external-root model
lifetimes. It does not qualify physical input, full O256 pressure, configured-cap
child/staging fanout, 1,000 Hz/release timing, native scoped/stationary behavior
or complete U7/U8. The separate Linux-only handler-forwarding correction is not
included in these gates.

The subsequent Linux-only physical harness migration preserves the inner close
disposition, forwards property-command retirement and rejects Retired pointer
commands as unapplied. Its actual Linux build and 29 unit tests pass; bounded
Xvfb/Openbox redock completes with resident/reservation peaks of 3,005,827 /
4,123,712 bytes under 4 MiB. At this checkpoint the XTest journey fails before release
because the production native preview is missing after observed press,
threshold, routed outside motion and pass-through ACK. This physical blocker
remains open; callback migration does not certify U7/U8.
Subsequent minimal tracing directly identifies the premature cancellation:
ordinary fresh-roster recapture has both fixed backings borrowed with two
retained records and 2.67 ms oldest age, while byte/raw/output checks pass.
That transient shortage currently freezes ingress and commits InputOverload.
The subsequent roster-backpressure slice preserves the gesture through the
existing protected roster receipt and prepaid root control outputs. A borrowed
backing requests one coalesced fresh recapture; true window-capacity exhaustion
still freezes admission. The first abandoned ordinary output also suppresses
semantic/application work. No third payload or relaxed cap is introduced.
All 11 roster protocol tests pass. The unchanged real X11/XTest driver now
completes drag-out preview-before-release, child first-live, Center-guide
drag-back preview-before-release, exact DestroyNotify, inventory removal and
quiescence under Xvfb/Openbox/software GL. Frozen identities and the physical
log are retained in `/tmp/dockspace-roster-backpressure-snapshot.json` and
`/tmp/dockspace-roster-backpressure-physical-x11.log`. This closes the specific
missing-preview blocker; non-guide-point and mixed-DPI certification remain open.
The same frozen slice passes native 444/444 normal/assertion-disabled tests,
including both same-instance 10,000-lifetime model runs, Clippy, strict docs,
default/bounded macOS redock and all three macOS/Linux O256 pressure journeys.
The physical journey and delayed-output journeys remain separate evidence.

The next output-pressure slice uses a dedicated real-window binary with actual
eframe token/result callbacks and a bounded 258-result test proxy. After one
real child staging request is waiting, the proxy defers that candidate, pauses
new complete inventory capture and fills the independent O256 owner backing.
It proves root control and protected child staging admission with their exact
NotPresented/Presented terminals, then requires root Defer before returning the
original FIFO. Fresh inventory resumes on the next framework candidate; normal
first-live/redock/retirement and a final zero-output-owner boundary are required.
Duplicate and reordered terminal journeys run in separate processes and must
retain the exact order-violation fatal while completing original cleanup without
extra errors. The selected I64/W2/R258/E2/M4/C2/L2 configuration remains under
4 MiB; proxy storage is measured separately. This isolates O256 and does not
replace the full-roster/configured-cap cross-saturation gate.

The implementation also repairs consumed repaint acknowledgement after native
Defer, repeated full-owner prefix preparation, and quarantine's order-error /
accepted-staging cleanup barriers. Prefix membership uses bounded original
tokens and ordinals; its overlapping fixed scratch is charged to the existing
owner allowance. Consumed native wakes preserve real-pass repaint history, and
root close passes retain their framebuffer without issuing post-exit semantic
output obligations. The final terminal and window-exit checks require no
remaining output owner. Native application construction rejects profiles with
no root resource or close-request slot before provider enable.
Fresh qualification passes native 443/443 normal/assertion-disabled tests,
fork Glow196/WGPU190/no-assert196, both renderers without the seam, Clippy,
strict docs, default/bounded redock and all three macOS pressure journeys.
The pressure journeys sample 2,843,172 resident / 3,807,664 reserved bytes below
4 MiB, with a separately bounded 18,576-byte proxy. The bounded redock samples
3,005,859 resident / 4,155,496 reserved bytes. A disposable-snapshot negative
control removing only consumed-wake acknowledgement fails; exact restoration
passes. The local fork is committed at `86feb7f8cc49217bff54a8924b2cb525f4af3285`
without changing the earlier remote manifest pins.
The same source subsequently passes all three pressure journeys on Linux/arm64
under Xvfb/Openbox/software GL, sampling 2,843,140 resident / 3,808,176 reserved
bytes under the same 4 MiB profile. This confirms real-window delayed output
delivery on that backend without claiming physical pointer qualification.
Staging without a captured terminal at fatal,
the broader physical matrix, 1,000 Hz/release latency and native scoped/stationary continuation
remain explicit work. No complete U7/U8 claim follows from this slice.

The adjacent native prerequisite measures actual raw backing capacity and
removes work-area/roster conversion clones. Unknown work-area settlement
preserves token history while revoking routes. The frozen local source passes
306 native tests normally and without native debug assertions, Clippy and a
macOS programmatic real-window redock; this does not qualify byte admission or
physical input.

Low-frequency semantic ingress is now boxed so the largest `EngineInput` no
longer inflates every pointer payload allocation. The arm64 artifact's fixed
payload data decreases from 584 to 248 bytes. The native query includes the
separate semantic box while retry batches share it; this does not impose the
unfinished byte cap.

The facade also reclaims committed transport payloads before its next recording
operation, preserving recorder backing capacity. Caller-owned batch iteration
finishes before maintenance, and suspended atomic candidates skip reclamation.
The committed-256/next-256 and atomic-257 continuation checks pass with all 1,748
workspace tests and Clippy. Remaining capacity work must still bound other
prelude/fanout growth and protected variable payloads.

Raw callback storage is now preallocated for 4,096 entries and cannot expand on
push. Ordinary and terminal rosters share two preallocated 256-window/64-display
backings with exact borrowed fact deduplication; a busy pool freezes admission
and closes the current output as diagnostic without minting another roster.
Route/resource/retirement lifetime union admission reserves a complete window
slot before create or binding. Display overflow revokes only work-area authority
to Unknown without dropping window facts or backing capacity. Shared core/replay
byte allowance, prelude/fanout growth, load and soak are still unfinished.
The frozen source passes 316 native tests normally and without native debug
assertions, Clippy and a macOS programmatic real-window redock. Removing the raw
push guard fails the overflow regression; restoring it passes. A quarantine
DeviceRemoval journey commits both preexisting mouse/touch stream terminals
through the production gate. This is not a complete 4 MiB or physical-input
qualification.

DeviceRemoval no longer expands into per-stream synthetic pointer/scroll
records. One exact-provider backend fact invokes the shared live-input reset
kernel, preserves pointer sequence and accepted release/output obligations,
keeps the frame open for subsequent input and supports fresh roster recovery.
Adapter state is forgotten only after recording succeeds. Destroy-binding
fanout is superseded by the single binding-terminal fact above; caller atomic
groups and remaining variable terminal data still need shared work admission.
Qualification passes 1,753 workspace tests, 79 public-host tests, strict docs
and Clippy, 316 native tests in both normal and no-debug-assertion builds, and
the macOS programmatic real-window redock. A real presented-receiver regression
also reuses the same pointer after removal in one backend batch and requires a
new stream incarnation. Removing only DeviceRemoval staged-ledger sync fails
that regression with ButtonAlreadyPressed; restoring the implementation passes.

### U8. Consolidate optional viewport hosting and certify native readiness

- **Goal:** Preserve core native semantics while making resource ownership engine-implementable.
- **Requirements:** R6, R11, R13-R15, R18-R22; KTD7, KTD16.
- **Dependencies:** The resource-ownership prerequisite needs U15's native slice and the existing exact lifecycle contracts; native readiness and certification need completed U7.
- **Files:** core native transfer/admission/runtime modules, `crates/egui_dockspace_native/src/`, public host harness, `integration/egui-native-smoke/`, native fork seams and conformance tests.
- **Approach:** Join per-binding/incarnation host sidecars into one resource record, keeping semantic transfer in core. Remove universal assumptions about exact eframe hidden-stage passes. Make first-live the single durable transfer boundary and preserve failure recovery.
- **Resource-ownership prerequisite:** Consolidate exact physical-window/create and command/output reservations before adding U7 protected terminal admission. Reserve finite terminal storage when the obligation is established; late results consume that existing slot exactly once and cannot mint another owner. Root control outputs must still run the shared budget and close the current output token while ordinary admission is frozen. Retained diagnostic control passes must not continuously append new ordinary output records. This slice does not certify U8 or bypass first-live, close, replacement or quiescence verification.
- **Command and window identity:** Focus, visibility and pointer pass-through results must echo the exact host command identity; ordinary framework commands cannot settle those owners. Scheduling a typed property operation must not leave an unowned ordinary property command which can survive window retirement. Physical-window terminal reservations outlive logical route suppression when an OS obligation remains; reusing a viewport for a successor cannot overwrite or satisfy the predecessor's terminal slot. Reuse the existing create-attempt, held-close and retirement owners rather than introducing another lifecycle authority.
- **Test scenarios:** Covers AE6. Hidden or directly prepared backends, delayed first valid output, A1/A2 replacement, no-frame, pass-through, close/veto/defer, failure recovery, redock, retirement and quiescence preserve one owner. Qualified X11 physical input docks at non-guide points and retires the child.
- **Verification:** Public host lifecycle traces, programmatic real-window tests, and qualified X11 physical journeys pass; old distributed maps and obsolete staging contracts are removed.

Local resource-ownership checkpoint (2026-10-29): command/output reservations
and deferred-window sidecars now have joined owners. An admitted create attempt
also retains one non-Clone success/failure receipt; failure moves it from the
route into the existing unmaterialized retirement entry. Raw handoff and partial
commit do not release it, and an unrecorded failure result cannot reach
quiescence. HostRecords owns the sole retirement state and the complete frame
commit hook. This native work is locally qualified against the unpublished fork
and committed locally with the adapter. The physical-window follow-up reserves
one non-Clone Destroyed receipt on attach and moves it to the same retirement
owner across route displacement, unbind and release. Admission checks both live
and retired binding/WindowId ownership, and terminal receipt commit gates
tombstones and quiescence. A same-window rebind is allowed only before Destroyed
capture; old pointer-reset and semantic tombstone ownership cannot be reassigned
afterwards. Held-close ingress now captures into the sole HostRecords close
owner before Hold. Its cancellation receipt commits only at a complete frame
boundary plus successful semantic Clear; one fixed successor retains the original
journal reservation until then. The duplicate window-resource close slot and
lifecycle close counter are removed. Exact command receipts now hold their
callback envelopes inline, preserving capture order and age across route
retirement and A1/A2 replacement without another terminal queue. Queued property
commands retired before execution settle those same owners through the local
fork's exact Retired result. Fatal-quarantine roster rearming inspects both raw
and owner-held callbacks. Fresh native gates pass 271 cases normally and 271
without native debug assertions; 63 fork host-seam tests, WGPU compilation,
independent production review and programmatic macOS redock qualify this local
ownership slice, not a real canonical Hold/Veto input journey. The required fork
commit `1f7c541fde99fd69192622a1a36cb642de58ed29` remains unpublished and tracked
dependency pins unchanged. Protected overload admission and the U7/U8 exit
criteria remain open; these checkpoints do not establish native readiness.

The next local U7 storage prerequisite moves output, staging and close
cancellation envelopes into their existing inline resource slots. Close
successors become ready inside their original owner rather than re-entering the
ordinary queue. Output submission and partial replay retain the same ordered
head until full commit. Create/Destroyed storage and unified route/resource
observation still precede final protected admission; these changes do not
establish the record/byte/age caps or fixed-boundary cancellation.

The single-state follow-up joins route and resource observation under one
private host mutex, removes separately shared route handles, and performs
staging/no-frame checks on that same state. Existing receipts retain sole
ownership. Local native compatibility passes 277 tests normally and 277 with
native debug assertions disabled. Create/Destroyed storage and the final U7
admission scheduler remain subsequent work.

The create/physical-window follow-up stores their callback envelopes inline on
the original receipts and joins live and retired receipt ingress into the same
ordered head and count/age/binding retention. Destroyed capture does not create
a retirement entry; an exact Received preflight precedes its reducer-side move.
Pointer suppression is inline on the route. Created window index insertion is
also deferred to its reducer, with exact route fallback and peer exclusion in
the interim. Complete receipt commit requires both semantic transfer and
original envelope handoff. This completes the existing owner terminal-storage
prerequisite; fixed-boundary admission, capacity, load and the U7/U8 exit gates
remain open. Variable-size terminal-roster storage must also be reserved and
accounted before final protected-admission qualification.

### U10. Complete layout management and a bounded editor showcase

- **Goal:** Validate realistic editor use without building the user's engine.
- **Requirements:** R7, R8, R17-R19, R24, R25; KTD11.
- **Dependencies:** U12; optional native showcase paths also require U8.
- **Files:** core layout/persistence commands and tests, `crates/egui_dockspace/examples/`, `crates/egui_dockspace_native/examples/`, product persistence and overflow harnesses.
- **Approach:** Finish reopen/closed inventory, lock/reset/defaults, application-owned named presets, overflow ownership, focus recovery, and an editor-shaped egui showcase. Use representative text/scroll/canvas content; do not introduce engine/ECS or another docking implementation.
- **Test scenarios:** Restore with missing panes or changed surface geometry, close/reopen, preset changes, layout lock, overflow owner transitions, and focus recovery preserve stable identities and expose typed outcomes.
- **Verification:** Public commands and representative egui workflow agree; native features are labelled by actual backend capability.

Local U10 implementation checkpoint (2026-10-28, remaining acceptance open):

- Reuse `OpenItem` for reopening stable application catalog entries. New content
  selects and requests the existing pane-focus intent; `Existing` remains inert.
  Missing anchors reject explicitly, with fallback chosen by the shell from the
  current public view rather than an engine-owned placement-history sidecar.
- Arrangement locking is an independent revisioned policy restriction. It blocks
  content membership and layout geometry edits while preserving navigation,
  content input, focus and permitted contained raise. Lock/unlock preserves all
  configured item/source/surface rules. Live replacement requires a committed
  unlock; irreversible native teardown/recovery retains its existing authority.
- Pointer tab selection is independent of docking-source eligibility. A
  policy-denied single tab may select/focus without arming a drag; group/title
  drags and other errors remain rejected. Required contained raise still obeys
  its original rules.
- The persistence example supplies an eight-pane editor shell, application-owned
  closed inventory, immutable default and eight user preset slots, with text,
  scroll and drawn content. It resolves restored surface/root identities from
  the current view and retains application pane data across close/reopen/reset.

The subsequent restore slice preserves semantic focus across shared live
replacement with one successor intent, a deterministic selection/workspace
fallback, rollback and old-observation rejection. Native destinations wait for
fresh eligible bindings; mixed logical/native surfaces retain their distinct
scope. Known keys with temporarily unavailable pane content keep their layout
identity and render recoverable placeholders; key rejection still rejects the
whole document. Product acceptance also covers locked overflow navigation and
contained raise/focus with protected geometry.

The layout-only reset slice adds an opaque, session/version-bound prepared
value and standalone/native reset-frame entries without requiring serde or
document lineage. Reset and document restore reuse one replacement phase and
ordered native prefix. A dropped frame retains the same preparation for retry;
success invalidates it. Reused defaults rebase retired surface/root/floating
identities after reserving every incoming fresh identity, keeping application
items, selections and rectangles intact. A document-bound reset may reuse
registered closed items but cannot introduce unknown numeric items or replace
key history, placement sidecars or save generation. The egui facade and Default
button use this core entry. Measurement installs Ready scenes at commit for
painting in a later frame; it does not grant an immediate replacement paint.

The optional native showcase and complete overflow-owner acceptance remain
open. These local checkpoints do not complete U10. Qualification for the
layout-only reset is recorded separately in `docs/VALIDATION.md`.
The local overflow slice adds active-drag edge scrolling through the existing
ordered pointer kernel. Edge scrolling and hover selection occupy one mutually
exclusive continuation; a step uses fresh current-output clearance, excludes
close/control receivers, writes through the shared tab-strip offset boundary and
waits for exact presentation of the new geometry. Manual scrolling and menu
actions revoke timed work; smooth manual ownership blocks reacquisition until
its terminal boundary. Product journeys cover automatic scroll, explicit strip
scroll, menu selection and selection reveal, with independent public-host
stationary, retry, terminal-order and receiver-proof checks. Deferred renderer
results wake an idle official-egui host once for acceptance. Native stationary
routing and the native editor showcase remain open; full U10 readiness still
requires their stated qualification. See `docs/VALIDATION.md` for current gates.

### U11. Certify Windows and macOS capability claims

- **Goal:** Add verified native platforms without delaying generic core delivery.
- **Requirements:** R6, R14, R15, R18, R20, R23; KD4.
- **Dependencies:** U8.
- **Files:** Winit/eframe platform seams, native capabilities, physical smokes, `.github/workflows/ci.yml`, `docs/PLATFORM_SUPPORT.md` (new), seam admission documentation.
- **Approach:** Prove each platform's event-time coordinates, hover/capture/release, lifecycle, work area, and mixed-DPI mappings independently in a stable CI or recorded release environment.
- **Test scenarios:** Same-window and cross-window drag, first-live, redock, close, Retina/mixed-DPI, focus interruption, missing facts, and quiescence preserve the admitted lane and ownership.
- **Verification:** Enable each platform claim only after its own deterministic and real-input matrix passes. Wayland/global-position limits remain explicit; proven local operations remain available.

### U9. Close evidence, deletion, documentation, and native packaging

- **Goal:** Make repository claims match the final core and adapter contracts.
- **Requirements:** R1-R25; KTD9, KTD10, KTD12.
- **Dependencies:** U8, U10, U12. U11 evidence is included only for platform claims being enabled.
- **Files:** core/public/product/native tests, `.github/workflows/ci.yml`, `README.md`, `docs/VALIDATION.md`, migration/platform documents, module inventory and fork admission records.
- **Approach:** Reconcile public-host, egui, model, real-window, and physical evidence; remove superseded APIs/tests/helpers and update package boundaries. Preserve historical reference provenance without requiring Open-GPUI cutover.
- **Test scenarios:** Full local journey parity, assertion-disabled native operation, stable retained output, long-run lifecycle quiescence, public packaging, and stale-output rejection pass with the documented host capabilities.
- **Verification:** No obsolete terminal owner, hidden required host API, ignored release blocker, or fork-only requirement is marketed as generic core support.

---

## Verification Contract

Use `CARGO_BUILD_JOBS=1` for local Cargo builds and `--test-threads=1` when serial test execution is required.
Nextest's `-j` controls test concurrency; do not confuse it with Cargo compilation parallelism.
Reuse existing target directories and serialize builds when the machine is busy.

| Gate | Command or evidence | Proves |
| --- | --- | --- |
| Core | `cargo nextest run -p dockspace --all-features --all-targets --locked --test-threads=1` | Model, transactions, input, output identity and lifecycle semantics |
| Public engine consumer | Planned: `cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --all-features --locked --test-threads=1` | Public-only immediate/deferred integration and logical-surface independence |
| Core dependency boundary | `cargo tree -p dockspace --all-features` plus review of packaged/public API | R19 dependency independence; no compiler/source parser gate |
| Shared journeys | `cargo nextest run -p dockspace_host_conformance --locked --test-threads=1` | Finite normalized behavior and honest oracle classifications |
| egui product | `cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --all-features --locked --test-threads=1` | Interactive reference behavior and exact supported boundary |
| Official egui | `cargo nextest run --manifest-path integration/egui-official-harness/Cargo.toml --locked --test-threads=1` | Crates.io adapter compatibility independent of forks |
| Native default | `python3 scripts/run_egui_fork_harness.py` | Fork-backed coordinator contracts |
| Native without debug assertions | `cargo nextest run --manifest-path integration/egui-fork-workspace/Cargo.toml --package egui_dockspace_native --lib --locked --test-threads=1 --no-fail-fast --config 'profile.test.package.egui_dockspace_native.debug-assertions=false'` | Required state updates execute in release-style assertion configuration |
| Native output/lifecycle | Existing programmatic smoke and qualified X11/XTest CI; later per-platform U11 environments | Actual backend lifecycle and physical input, separately labelled |
| Performance | Public-host structural fixtures; host ingress/lifecycle soaks under the Resource Budgets table | R13/R24 cost, latency, retained-state and quiescence |
| Docs and packaging | Strict core/adapter rustdoc, core tarball verification, adapter package contents and downstream builds | Documented host contract and usable package boundary |
| Patch integrity | `git diff --check`; formatting for affected workspaces | Changes remain reviewable and formatting gates pass |

A planned harness or platform document must be created by its owning unit before its command is claimed runnable.
The current official eframe callback-only path and caller-owned run path have different capability claims; test both honestly.
Manual output owners, egui discard/unwind, delayed engine records, stale identities, foreign overlays, and no-input settlement all exercise the same R11 boundary.
No test uses a traversal receipt, cumulative frame number, guide center, screenshot, or synthetic capability token as a substitute for the fact being tested.
Real-window and physical evidence is never inferred from a recording renderer.

---

## Definition of Done

- The public `dockspace` host contract supports an engine-owned UI/render loop without any UI/platform framework dependency.
- Immediate and owned/deferred consumers use the same semantic program, stable identities, exact output lifecycle, and core manipulation semantics.
- Complete same-tick gestures, non-guide center/edge docking, full contained content projection, cancellation, stale rejection, close, feedback, and focus intent pass ordinary gates.
- One manipulation owner and one presentation pipeline replace old local/journal/contained terminal paths and public preview-only ACKs.
- Local operations work without desktop facts; optional native input and lifecycle are bounded, causal, incarnation-safe, recoverable, and quiescent.
- First-live publishes durable ownership once; fixed host staging passes are not mandatory for every engine backend.
- egui 0.37 validates the adapter contract, with exact documented limits for ordinary eframe, caller-owned output, and fork-backed native hosts.
- Native capabilities are advertised only for proven platforms; Open-GPUI integration and the user's actual game engine do not gate this work.
- Required public host APIs are documented, packages are usable downstream, and obsolete APIs, duplicate state owners, stale fork seams, and misleading acceptance claims are removed.

---

## Appendix

### Research Baselines

- Core baseline: `3216855` plus the existing uncommitted U2b work; 2026-10-27 audit evidence is recorded in `docs/VALIDATION.md`.
- Updated ImGui: `d88f238d1d363ef387382b2e29ad12cf96f826ac`, `repo-ref/imgui/imgui.h` (`ImGuiViewport`, `ImGuiPlatformIO`), `imgui_internal.h` (`ImGuiDockNode`), `imgui.cpp` (docking and viewport update), and platform/renderer backends.
- Existing native dependencies: egui fork `660073444296f40903e92dbce0fb818a7f4771b0`, Winit fork `f378b50f29cafafec31cd8675c17646b4ef57d70`. These remain current pins until U15 changes them together with validation.
- The dirty `repo-ref/egui` checkout is not the fork currently selected by native manifests; `repo-ref/egui-release` is the reviewed pinned source.
- Upstream egui 0.37.0 release, workspace manifest, core/epaint/egui-winit/egui-wgpu/eframe changelogs, `Context` and `Plugin` sources were inspected for U15.
- Exact upstream sources: `https://github.com/emilk/egui/releases/tag/0.37.0`, `https://github.com/emilk/egui/blob/0.37.0/CHANGELOG.md`, `https://github.com/emilk/egui/blob/0.37.0/Cargo.toml`, and the corresponding per-crate changelogs.
- Earlier design context: `docs/plans/2026-08-08-001-refactor-dockspace-product-boundary-plan.md`, `docs/plans/2026-08-18-001-interaction-floating-multiview-conformance-plan.md`, `docs/knowledge/egui-native-fork-seam-admission.md`.
- Open-GPUI's current `spike/dockspace-core-adapter` already depends on this core and is not an independent behavioral oracle; historical algorithms may be consulted without resuming that adapter.
