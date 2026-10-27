---
title: Dockspace engine host architecture and direct manipulation - Plan
type: refactor
date: 2026-08-21
revised: 2026-10-27
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
| Hover selection | Optional 150 ms event-time dwell, reset by target/scene/gesture/output/policy change; never implies pane focus |
| Temporary suppression | Shift disables magnetic acquisition in the default editor profile; R25 arrangement lock remains independent |
| Local touch | 350 ms long press and 8-point slop; pre-arm movement yields to pane scrolling, early release activates/selects, armed stationary release opens a semantic context menu |
| Resize and controls | Eight contained directions; corners outrank edges; 8-point mouse edge band, 12-point corners, and at least 44-point touch targets clipped away from title/tab controls |
| Splitter commands | 16-point keyboard/accessibility adjustment; double-click equalizes only the adjacent pair while preserving their combined share |
| Overflow completion | Under R25, one owner at a time: selected-tab reveal, manual scrollbar, active-drag edge scroll, or menu selection; clipped subcontrols cannot execute |

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

As checked on 2026-10-27, the implementation pins egui/eframe 0.36.1 and Winit 0.30.13.
The selected stable egui target is 0.37.0, released 2026-10-15, with Rust 1.95 still matching this workspace.
Winit stable is still 0.30.13; prerelease 0.31 work is not a prerequisite.
The updated local egui_tiles checkout is reference material; it is not a runtime dependency.

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
  U14[U14 current correctness repairs] --> U13[U13 public engine host contract]
  U14 --> U15A[U15 official egui 0.37]
  U13 --> U2[U2 emission and presentation closure]
  U15A -->|egui slice| U2
  U2 --> U3[U3 one manipulation kernel]
  U3 --> U4[U4 magnetic and full content projection]
  U3 --> U5[U5 public host blocker port]
  U4 --> U6[U6 commands facade feedback]
  U5 --> U6
  U6 --> U12[U12 core readiness]
  U15A --> U12
  U12 --> U10[U10 layout shell and showcase]
  U3 --> U7[U7 bounded ingress and handoff]
  U5 --> U7
  U15A --> U15B[U15 minimal native fork rebase]
  U15B --> U7
  U7 --> U8[U8 native runtime readiness]
  U8 --> U11[U11 per-platform certification]
  U12 --> U9[U9 evidence and deletion closure]
  U10 --> U9
  U8 --> U9
```

U1's finite journey/oracle work continues as evidence for the behavior each replacement touches; existing valid cases are reused rather than restarted.
Core readiness (U12) means public engine integration plus egui local validation, without waiting for Open-GPUI or every native backend.
U5's common host port and truthful official-egui boundary satisfy U6/U12; exact fork-backed native hit parity is completed with U7/U8 after the native U15 slice.
Native readiness (U8, then U9) adds the optional core host lifecycle and qualified X11 reference.
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
- **Approach:** Document and expose the necessary host facade. Make output records usable for immediate consumption and compilation to owned host work, remove widget-specific SourceGap obligations, and decouple logical-surface routing from native registration. Add a tiny downstream consumer and recording renderer through the same supported API.
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
- **Dependencies:** U13 and the relevant U1 journeys. Changes to the egui bridge additionally require U15's official slice; independent core work can proceed before it.
- **Files:** `crates/dockspace/src/scene.rs`, `crates/dockspace/src/scene_compiler/`, `crates/dockspace/src/runtime/{paint,presentation,host_frame}.rs`, `crates/dockspace/src/{semantic_manifest,presentation_hit}.rs`, `crates/egui_dockspace/src/{renderer_pass_bridge,pass_settlement}.rs`, public host harness and existing presentation tests.
- **Approach:** Retain U2a's canonical traversal and finish U2b's exact output commit path. Give execution receipts R11's limited meaning, remove public preview-only acknowledgement authority, and make receiver/semantic/program promotion one committed artifact. Treat mutable egui shape markers as adapter-local diagnostics unless their limited acceptance contract is explicitly proven.
- **Test scenarios:** Covers AE5/AE13. Complete/failed emission, dropped output, output-hook modification, unwind, late predecessor, failed core commit, and idle post-commit settlement never create mixed artifacts or release authority.
- **Verification:** One exact output path is used by public engine, egui, and native integrations; flat schedule reconstruction and preview-only acknowledgement bypasses are deleted.

### U3. Converge ordered manipulation and terminal state

- **Goal:** Make a gesture's complete lifecycle local to one deep core module.
- **Requirements:** R1-R4, R8, R9, R11, R14, R17; KTD3, KTD8, KTD10.
- **Dependencies:** U2.
- **Files:** `crates/dockspace/src/runtime/direct_manipulation.rs` or a focused module directory, `crates/dockspace/src/interaction.rs`, `crates/dockspace/src/engine/{local_response,pointer_transaction,local_contained,pointer_contained}.rs`, `crates/egui_dockspace/src/product_render/actions.rs`, public-host and product gesture tests.
- **Approach:** Normalize ordered input boundaries and semantic commands into one transaction owner. Keep semantic controls/scroll separate where their existing subsystem owns them. Remove old release/cancel/ACK terminal reducers rather than forwarding through them.
- **Test scenarios:** Covers AE2/AE4. Same-tick press/move/release, duplicate release, cancellation, capture/source/target loss, discarded preview, and policy changes produce one terminal outcome through both input adapters.
- **Verification:** Complete Burst cases enter ordinary passing gates; one core release barrier and cancellation owner remain.

### U4. Complete magnetic behavior and live contained content

- **Goal:** Deliver consistent direct manipulation through the host-neutral projection.
- **Requirements:** R2-R5, R8, R10, R17, R24; KTD2-KTD5.
- **Dependencies:** U3.
- **Files:** `crates/dockspace/src/drop_resolver.rs`, `crates/dockspace/src/scene_compiler/surface.rs`, `crates/dockspace/src/runtime/direct_manipulation.rs`, `crates/egui_dockspace/src/product_render/`, public-host and product projection tests.
- **Approach:** Apply transient placement before compiling contained chrome and pane subtrees. Preserve one target solver, source suppression, event-time dwell and frozen pending-release plan. Use cached valid measurements where possible.
- **Test scenarios:** Covers AE2/AE3. Non-guide basin samples, jitter, hysteresis-edge release, contained move/eight-way resize, target panes behind the source, and cancellation compare program, hit, semantic and durable results.
- **Verification:** Full pane content follows the projection; a window-sized empty ghost is insufficient. Structural cost meets R24.

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
- **Dependencies:** U3, U5, U14, and U15's native slice.
- **Files:** `crates/egui_dockspace_native/src/{mailbox,coordinator,pointer_event,input_control,surface_driver}.rs`, core ingress/pointer modules, native coordinator tests, public-host scoped-input tests.
- **Approach:** Replace one-callback-per-cycle front/ACK families with bounded immutable prefix claims. Establish surface-local routing and validated desktop handoff through the same core kernel; preserve exact output and lifecycle barriers.
- **Test scenarios:** Covers AE7/AE11/AE14. Retry after a rejected batch, 10,000 interleaved records, sustained 1,000 Hz drag, overload, local touch coordinates, and scope/provider changes preserve one terminal release.
- **Verification:** Resource budgets hold and no unbounded journal or forced desktop-only local route remains.

### U8. Consolidate optional viewport hosting and certify native readiness

- **Goal:** Preserve core native semantics while making resource ownership engine-implementable.
- **Requirements:** R6, R11, R13-R15, R18-R22; KTD7, KTD16.
- **Dependencies:** U7.
- **Files:** core native transfer/admission/runtime modules, `crates/egui_dockspace_native/src/`, public host harness, `integration/egui-native-smoke/`, native fork seams and conformance tests.
- **Approach:** Join per-binding/incarnation host sidecars into one resource record, keeping semantic transfer in core. Remove universal assumptions about exact eframe hidden-stage passes. Make first-live the single durable transfer boundary and preserve failure recovery.
- **Test scenarios:** Covers AE6. Hidden or directly prepared backends, delayed first valid output, A1/A2 replacement, no-frame, pass-through, close/veto/defer, failure recovery, redock, retirement and quiescence preserve one owner. Qualified X11 physical input docks at non-guide points and retires the child.
- **Verification:** Public host lifecycle traces, programmatic real-window tests, and qualified X11 physical journeys pass; old distributed maps and obsolete staging contracts are removed.

### U10. Complete layout management and a bounded editor showcase

- **Goal:** Validate realistic editor use without building the user's engine.
- **Requirements:** R7, R8, R17-R19, R24, R25; KTD11.
- **Dependencies:** U12; optional native showcase paths also require U8.
- **Files:** core layout/persistence commands and tests, `crates/egui_dockspace/examples/`, `crates/egui_dockspace_native/examples/`, product persistence and overflow harnesses.
- **Approach:** Finish reopen/closed inventory, lock/reset/defaults, application-owned named presets, overflow ownership, focus recovery, and an editor-shaped egui showcase. Use representative text/scroll/canvas content; do not introduce engine/ECS or another docking implementation.
- **Test scenarios:** Restore with missing panes or changed surface geometry, close/reopen, preset changes, layout lock, overflow owner transitions, and focus recovery preserve stable identities and expose typed outcomes.
- **Verification:** Public commands and representative egui workflow agree; native features are labelled by actual backend capability.

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
