# Current layout work after the main merge

The current source still allocates a fresh continuation stack for every root. Removing that stack through graph-owned traversal events is a concrete, different structural hypothesis; changing its container or immediately popped transitions would repeat earlier experiments. Explicit cross-root scratch reuse is also possible, but it cannot affect the frozen formal caller without an API/caller migration. On the present binary workload, most of the named scratch Vecs never allocate.

The new source-matched diagnostic arrived during this review. Its timed stack free is identifiable, but one sampled allocation initially suggested as stack storage is actually split-outcome storage; the exact distinction is recorded below. Compiled events are a valid further hypothesis, not an implementation I recommend starting ahead of the newly located, smaller command-path opportunity. Do not use the old 49baba1 stack-growth sample as evidence about this retained binary, and do not claim a predicted percentage.

## Scope and bindings

Read `/tmp/dockspace-imgui-twofold-20261107-ea4161e/post-main-continuation/scope.json`. Its base is `c7508eec3ac44138305797691244ae9c180d7680`; the retained runtime remains `0e441c8`, with 30/42 complete-operation comparisons at most 2× ImGui and twelve misses at 16 panes. The scope freezes the formal probe, timers, reference implementation, and ImGui wrapper.

Source inspected:

| Path under `crates/dockspace/src/` | SHA-256 |
| --- | --- |
| `layout.rs` | `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` |
| `layout/topology.rs` | `1c0bd28862711316cbe9f963e46133a7f6440e774fc6686e5a1c056f626c94cb` |
| `layout/projection.rs` | `fb6fb6454034656114bfdcb80049f53fbbc325a780af00f720efd7769fa1494e` |
| `docking_cost.rs` | `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3` |

Only static source, the scope, and existing retained/rejected experiment records were consulted. No other agent's report from this round was read.

## What actually owns and allocates

`project_root_with_overrides` validates overrides, borrows the completed graph structure, freshly measures every leaf, and passes owned node values into projection (`layout.rs:488–524`). Measurement and the final node result already use the same Vec. There is no second full measurement Vec to remove.

| Owner/work | Current path | Applicability to the binary formal matrix |
| --- | --- | --- |
| Fresh leaf input Vec | `docking_cost.rs:197–250` | One per root, grown during actual graph traversal. Frozen formal input work. |
| Fresh `Vec<NodeResult>` | `layout/topology.rs:141–195` | One per root; constraints composed postorder, rectangles written during projection. Durable output. |
| Split outcomes and child records | `layout/projection.rs:215–220` | Two fresh Vecs per nonleaf root. Every min/max/actual/gap remains materialized. Durable output. |
| Continuation stack | `layout.rs:988–989` | One capacity request per root, bounded initial reserve, normal spill. Temporary. |
| Measurement child gather | `topology.rs:148–185`; delayed maxima at `layout.rs:617–647` | Binary cases use two local records. The fallback Vec allocates only for other arities. |
| Axis constraints, weights, sizes | `layout.rs:1066–1105,1127–1137` | Binary cases use local arrays. Constructing the default scratch Vec headers does not allocate payloads. |
| Eligible indices | `layout.rs:1394–1438` | Populated only during correction rounds. Exact-assigned paths skip those calls. |
| All-root projection Vec | `docking_cost.rs:221–256` | Fresh collection retaining every rich root output. Frozen formal work. |
| Fresh normalized export | `docking_cost.rs:329–350` | One owned word Vec; preorder joins still verify actual node identity and graph contents. Frozen formal work. |

For an unchanged binary 16-pane root, the structure has 31 nodes, 15 splits and 30 edges. Four views instead have four seven-node roots, each with three splits and six edges. The output buffers and stack therefore incur per-root construction even though the total node count is slightly smaller. These are source-level owner counts, not allocator-call counts or timing shares: input Vec growth can make multiple requests, and the compiler/allocator determines the actual implementation.

The complete layout phase includes both leaf-input collection and projection. A sample under `project_all_roots` alone does not identify which of those costs dominates. `measure` retains capture, checked mutation, all-root projection and complete fresh export at `docking_cost.rs:441–468`; fixture creation and later disposal have different ancestry.

## Current diagnostic: precise owner correction

Read the completed `post-main-continuation/current-main-profile` receipt, attribution and existing diagnostic executable. The receipt binds this runtime, identifies only the probe's iteration-cap assertion change, and records source restoration before execution. The core source hashes in the diagnostic and formal receipts match. Diagnostic SHA-256: `35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590`; receipt: `ec94fad210c92a69b6d0d86060bcf40b592542f7a0857f81353a159fc13a0f01`; attribution: `7604545a1451db79c66664bedf2f5d8b7b7299cf4f7286042a73bfd509a94962`.

Static `nm` places `project_measurements` at unslid address `0x100b37808`. Correcting the sample's image slide, the important return addresses have different owners:

| Function offset | Static evidence | Owner |
| --- | --- | --- |
| `+1632` / `0x660` | Read the root's split-row count, shift by five, allocate; subsequent initialization advances 32 bytes per record. Pointer stored at stack slot `0x108`. | Fresh `SplitOutcome` Vec, **not** continuation stack. |
| `+1796` / `0x704` | Read child-edge count, shift by six, allocate and initialize child records. | Fresh `ChildProjection` Vec. |
| `+1984` / `0x7c0` | Clamp node count to eight, multiply by 48, allocate; capacity/pointer stored at stack slots `0x188/0x190`, followed by the initial Enter record. | Continuation stack. |
| `+6564` / `0x19a4` | Free the pointer loaded from slot `0x190`, after checking capacity at `0x188`. | Continuation-stack disposal within projection. |

The current sample therefore supports a narrow location finding for temporary stack disposal and a source-bound stack allocation site. It does not turn every allocation under `project_measurements` into scratch. The attribution reports 170/142/177 inclusive timed-layout samples for four-view resize, single-view center and single-view edge respectively, with no time-share interpretation. The leaf collection and its growth are separately visible. No allocation percentage, predicted saving, or theoretical lower bound follows.

## Cross-root scratch: real but constrained

The natural deep Module would be an operation-local projector owning only the continuation Vec and nary/eligible scratch. Its Interface would project one root at a time; each result would continue to own its three fresh result buffers and immutable structure reference. No leaf answer, result, issuer, central route, or “validated” bit belongs in the scratch.

Reset the stack and root-local traversal state before every projection, including after a previous failure. Nary sizes must still be freshly initialized, eligible indices cleared before use, override validation and leaf callbacks remain in their original order, and delayed-minimum `RootMeasurement` keeps its independent values. The scratch lasts only for the encompassing operation, retaining the largest capacity until that operation ends. It must not become a global, thread-local, Workspace-owned, or cross-frame cache.

On this binary matrix, reuse chiefly removes the second and subsequent roots' stack allocations. It does not eliminate four axis-scratch allocations per root: those allocations are absent already. On general nary layouts it can additionally amortize gather/axis/eligible capacities.

However, the current formal Adapter calls the free, stateless `project_root` independently inside its map closure (`docking_cost.rs:242`). There is no caller-owned mutable projector to thread through those calls. A wrapper that still constructs scratch per free-function call changes nothing. Hidden persistent state would violate the intended boundary. An explicit reusable API would require migrating this frozen Adapter, so it is not a directly measurable production-only treatment under the current scope.

The product compiler has a real sequential lifetime where an owner could be threaded: main then contained roots in `scene_compiler/surface.rs:131–217`, consuming each `RootMeasurement` at `744–781`. That is a separate product API experiment, not evidence of a formal-matrix gain. Keep this distinction explicit if Root later elects a mechanical Adapter migration under a separately declared scope.

## A different opportunity: compile structural continuation events

The graph already emits dense preorder nodes, postorder rows and parent-edge ranges in `graph/root_structure.rs:145–198`. Current layout rebuilds a dynamic traversal on every projection through `ProjectionFrame` push/pop (`layout.rs:980–1023,1190–1261`).

A graph-owned event sequence could replace that stack without changing the formal caller. A minimal model is one root-enter event and, for each parent edge, a child-enter and child-return event in DFS order. Each edge event identifies its parent row and exact parent-edge slot; child identity comes from the existing children directory. The plan contains structural positions only, never measurements, geometry, bounds, weights converted for a particular layout, or authority.

The required sequence is:

1. Enter parent: record its bounds, run the existing constraint/weight/geometry preparation and unique solver, store all child allocations and outcome.
2. Enter first child using the parent's original axis origin and the stored allocation.
3. Finish that entire child subtree, including its errors.
4. Return child: advance the parent's cursor result; construct and store the following gap only now.
5. Enter the next child after that gap. Repeat in the same child order.

This removes runtime frame storage/growth and frame construction/pop/dispatch. It does **not** remove the solver, validation, composed constraints, child extents, gap rectangles, rich owned outputs, or leaf reads. It is different from the rejected fused-transition treatment: that treatment retained the dynamic stack and reduced immediate round trips.

### Exact cursor reconstruction

`LogicalRect` stores min/max corners. Its `width()` and `height()` subtract those corners (`geometry.rs:152–223`). They are not the original requested extents and must not be used to reconstruct a cursor or splitter thickness.

There is a smaller exact observation. The child's axis maximum was constructed by the very same addition as the later parent cursor update: child-enter constructs `cursor + stored parent-edge extent`; child-return currently repeats `cursor += that extent`. Taking that child's stored axis **maximum** reuses the result of the identical addition. Likewise, gap construction computes `cursor + thickness`, and the current code repeats that addition immediately afterward; the gap's stored axis maximum can supply the next child's origin.

Use the maximum coordinate directly, rather than `min + width`, and retain the original cross-axis getters and `LogicalRect::new` construction order. Finite validation returns original coordinates without zero canonicalization. This reasoning covers signed-zero inputs as well as ordinary coordinates. A child cannot be revisited inside its own acyclic descendants, so its own root rectangle is not overwritten before its return event; raw DAGs still require occurrence-specific edges and last-write behavior.

If Root prefers a more conservative first implementation, child-return may instead perform the original addition from child minimum and the stored parent-edge extent. It still eliminates the stack; do not derive that extent from the rectangle's width.

### Thickness and cost are the main unresolved design tradeoffs

The old frame retains the exact compressed splitter thickness from `projected_splitter_geometry` (`1169–1187`). An event stream still needs that value when the gap is constructed after descendants.

The simple option is one private freshly computed thickness scalar in each split outcome. That introduces no new buffer or trust state, but can enlarge outcome stride and retained/clone cost from active-depth state to all-split state. Actual layout must be measured; do not assume spare padding. Recomputing `projected_splitter_geometry` at every return repeats checked sums/division and can create quadratic work at high arity. Inferring thickness from rounded rectangle extents or distributing “unused space” changes the contract.

The structural plan also costs memory and completion work. A tree with N nodes has `2N−1` events in this direct representation. Structural Item edits already rebuild affected root rows, but appending another event stream is new timed mutation work, not free setup. Center/selection and resize can share unchanged structure; edge changes must pay for its reconstruction. Old outputs must retain their original plan through the same immutable RootStructure lifetime.

Single-parent links alone do not cover the raw diagnostic compiler's shared subtrees. `compile_draft` completes unique rows postorder (`topology.rs:35–41,82–129`), while projection intentionally revisits shared occurrences. Either preserve the existing cfg(test) raw traversal Adapter with the same numerical helpers, or represent every raw occurrence explicitly; do not deduplicate those events by row. Expanding every DAG path in advance has its own memory risk. This is a required design decision before implementation, not a reason to weaken the raw oracle.

## Other apparent duplication and the cheapest next step

Output default initialization is source-visible: all node placeholders and split defaults are later overwritten. It is not a second independent owner that can simply be deleted. Node measurement writes postorder into preorder positions. Split child ranges and raw split ordinals also have different construction orders. Eliminating defaults safely would need new construction discipline or adapters; use no unsafe/uninitialized access and do not paper over gaps with debug-only checks. Without current location evidence this is lower priority than the explicit continuation lifetime.

Constraint sums/getters also appear more than once across composition, axis validation and projection, but the preceding two experiments and earlier rejected axis/child-row variants already establish that source-level deletion alone is not a retention argument. The fresh min/max composition, overflow priority, delayed-maximum pass, and complete export remain required. Do not resume those rejected mechanisms under new labels.

The completed source-matched sample now supplies the narrow location evidence above. Preserve its separation of leaf collection, result initialization, continuation allocation/dispatch, solver work, fixture work, validation, and output disposal. For this hypothesis, no additional broad profile or container sweep is necessary merely to reconfirm that the stack exists. Do not convert inclusive samples or owner counts into counterfactual percentages.

If Root later chooses the event direction, the falsifiable experiment is: exact leaf-call/error order and bitwise rich outputs against the unchanged reference; existing deep, raw-shared, delayed-minimum, late-gap and old-output cases; unchanged work accounting or explicitly documented new structural-plan work; and the complete fixed comparison matrix including mutation regressions. A gain confined to resize cannot hide an edge rebuild loss.

For now I support deferral. The proposed deletion is real, but extra structural metadata, extra per-split thickness storage and raw-adapter complexity are material counter-costs. An explicit cross-root projector additionally conflicts with the frozen formal caller. The smaller command-path content-check shortcut currently being reviewed by its owner should be evaluated independently first; this report does not review or pre-approve that change. There is no basis here to claim the event approach must win or that it is necessary to implement it immediately.

No candidate, repository, cache, or fork was modified. Existing diagnostic instructions were inspected statically with `nm`/`llvm-objdump`; no executable was run. No network, Cargo, test, runner, profile, or timing command was run.
