# Scene architecture review at 5484071

Date: 2026-11-06. Reviewed commit: `54840717a2454c957b13a667a939ddf8229e52e9`.

This is a read-only architectural review of measurement, scene generation, derived receiver views, and their authority boundary. No implementation, Cargo command, benchmark, network request, or Git mutation was performed by this lane. Source paths below are relative to the repository root. Fresh sampling mentioned below was collected independently by the root agent.

## Design judgment

There is still avoidable architectural work. The strongest current explanation is **a partially linked immutable scene represented as several separately comparable documents**, with stable identities used both as external names and as internal association keys. This leaves two different costs: unchanged frames can compare large immutable facts again, while a changed surface fans out into several complete derived views. The framework-neutral docking abstraction does not inherently require either cost.

The evidence does not support “the numerical solver is too complex,” “all the gap is validation,” or “Rust ownership forces this cost.” It supports changing the internal ownership seam: let one immutable compiled generation own its linked plan, pointer/touch view, semantic view, and executable receiver links; let output/authority wrappers bind that generation to a concrete presentation. Carry a row-bound measurement batch through the same seam. Do not solve this by adding more fingerprints, dependency manifests, or proof scripts.

This is an extension of working architecture already present, not a claim that the current code lacks linking or sharing. `SurfaceProgram` already links to typed record rows, `SurfacePlanScene` already holds all the relevant views, `HitGeometry` is already separately shared, and the latest DropTarget execution link is already installed once. What remains is making that shared, linked unit the identity and association unit of the hot path.

## What the measurements permit us to conclude

The current product comparison has a material gap, but it is not a renderer-free docking-core ratio. The renderer-free complete update also includes its recording sink, so subtracting that number from the product is invalid. Latest qualified renderer-free idle is about 1.63 ms and selection about 9.7 ms at 1,024 panes; neither number attributes time to this review's proposed changes.

The root agent's fresh `5484071` product samples provide an important constraint:

| Sampled case | Standalone numerical layout | Scene compiler | Scene validator | Hit/touch | Paint-output audit | Measurement/resources | Receiver queries |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| idle, 6,320 samples | 0% | 0% | 0.063% | see source attribution | 15.665% | 11.472% | 7.722% |
| tab click, 6,374 samples | about 0.8% | 10.056% | 4.534% | 5.632% | see source attribution | see source attribution | see source attribution |

These are inclusive sampled categories; categories overlap and sampling includes unmeasured preparation, warmup, hover, and disposal. They are not additive budgets and cannot be multiplied by frame medians to get exact savings. Inlining can hide work. They do, however, directly contradict an explanation in which ordinary idle is dominated by solving or full scene validation. Click requirement refresh was 2.714%, another constraint against treating any one small category as the whole problem.

In particular, the 15.665% paint-output-audit category includes `PaintAudit::check_append` closures that call real `PaneView::ui` and label drawing. It is not an exclusive audit-overhead measurement; removing auditing would not remove the nested application/content work. Any audit optimization claim must isolate the actual proof/capture work from those callbacks.

The fresh idle call tree also contains `reduce_tick_candidate -> SurfaceSceneSet::eq -> SurfacePlanScene::eq -> PresentationPlan::eq`, including DropTarget and layout facts, plus `HitGeometry::eq`. This is positive evidence that the equality path is executed in the current binary, not merely reachable dead code. Its exact frame contribution remains unqualified.

## Four current causes, ranked by confidence and actionable scope

### 1. Published-state equality descends into immutable scene contents

**Direct evidence, high confidence; exact speedup unknown.**

`engine/reducer.rs:936-945` decides whether published state changed by comparing `before_scene != candidate.presentation_authority.scene`, then computes surface deltas. `SurfaceSceneSet` derives `PartialEq` over its map (`scene/lifecycle.rs:710-715`); nested `SurfacePlanScene` derives it over the plan, hit manifest, paint links, semantic inventory, and coordinate capture (`scene/lifecycle.rs:117-126`). `PresentationPlan` derives it over its full facts and record inventories (`scene.rs:207-236`). `HitGeometry::eq` explicitly compares region and touch-fragment arrays (`presentation_hit/geometry_reuse.rs:20-23`).

This is distinguishable from the install reuse check, which already has a pointer fast path at `scene/lifecycle.rs:1234-1240`. Adding another install cache would miss the reducer comparison shown in the fresh sample. Arc-sharing a fact is not sufficient when another enclosing derived equality still compares the shared value.

**Necessary work:** determine whether published authority, candidate/fallback identity, coordinate capture, native provenance, or other mutable header fields changed. Equal scene facts separately constructed may need value equality in some public/testing contexts.

**Avoidable amplification:** comparing the contents of an already identical immutable generation to decide that the generation did not change. This does not require a content hash. A private identity comparison or a pointer-equality fast path on the shared immutable owner can prove this exact case. Keep value equality as an explicit diagnostic or fallback where its current semantics matter. Do not replace whole `SurfaceSceneSet` equality with plan pointer equality; its authority/header fields also matter.

This is the highest-confidence small experiment for the current idle path. It can be tested before any broad redesign.

### 2. The synchronous measurement owner uses the detached, keyed contribution protocol

**Direct evidence, high confidence; contribution to the gap not yet isolated.**

`runtime/measurement.rs:201-289` itself iterates frozen requirements and calls a value-returning resolver. The core supplies every key: pane rows at 233-247, intrinsic rows at 249-264, strip rows at 266-282. It then sends its own complete batch through `begin_surface_contribution -> prepare_surface_contribution -> compile_surface_contribution` at 284-289.

The batch is three `Arc<BTreeMap<Key, Measurement<Value>>>` collections (`scene_manifest.rs:1107-1115`). Insertion still detects duplicate keys through `entry` (`scene_manifest.rs:1230-1279`). Compilation validates exact required/submitted sets (`scene_compiler/surface.rs:452-453`; `scene_manifest.rs:1067-1103`), implemented as lookups from each set into the other (`scene_manifest.rs:1445-1458`). Authoritativeness then traverses the values (`scene_manifest.rs:1295-1340`), and the authoritative compiler accessors look up values by key again (`scene_manifest.rs:1376-1397`). Complete Ready reuse compares the new measurement map contents (`engine/surface_contribution.rs:285-295`).

This is **not** an assertion that snapshots deep-copy the maps: `Arc` cloning already shares them. The problem is materializing keyed answers and re-establishing the requirement/answer relation even when the only producer is the loop owning that relation.

**Necessary work:** call the resolver for every requested value; check answer variant, unavailable status, value/geometry validity, and current ticket/coordinate/policy authority. Stable first-error behavior is part of the current observable contract. Actual measurement-value comparison is still necessary when callbacks provide no stability promise.

**Avoidable amplification:** the synchronous producer cannot invent, omit, or duplicate the key supplied by its own loop except by returning early, so it does not need to serialize that relation into maps and prove the set relation again. Give it a private row-bound answer batch tied to the frozen request table. Detached/public submissions can retain keyed input and validate once into that same batch. This removes a protocol conversion, not any required measurement.

The complete Q must remain complete. `scene_compiler/requirements.rs:151-162` requests pane minima for **every item in a nonempty tabs node**, not only the selected item; strip/intrinsic requests follow policy at 184-214. Measuring only selected panes would change semantics and invalidate this proposal's comparison.

### 3. A new plan starts several complete surface-wide derived passes

**Direct evidence for fanout, high confidence; safe granularity and savings are hypotheses.**

Ready reuse requires the exact current inputs (`engine/surface_contribution.rs:285-344`). The code explicitly says local tab-state edits invalidate Ready before preparation at 334-337; a miss compiles/finalizes a surface at 362-377. Installation then compiles the hit manifest, links paint execution, and independently compiles the semantic manifest (`scene/lifecycle.rs:1264-1270`).

Hit compilation emits the program's regions, canonicalizes their order, compiles touch fragments, compacts rows, and freezes arrays (`presentation_hit.rs:430-457`). Its reuse attempt first compares complete touch-context inventories (`presentation_hit/touch.rs:78-126`), then re-emits the program to check each prior region (`presentation_hit/geometry_reuse.rs:46-66`). A failed attempt can therefore do replay work before the full compile path. Semantic compilation traverses the program independently and sorts/deduplicates another receiver inventory (`semantic_manifest.rs:25-106`).

**Necessary work:** a changed geometry dependency must produce correct pointer/touch geometry and precedence; semantic reachability is distinct from pointer reachability. Clipped menu rows remain keyboard/accessibility reachable (`semantic_manifest.rs:1-6`), so a semantic manifest cannot simply become an alias of the hit manifest. Touch compilation partitions expanded splitter/contained handles and subtracts occlusions/protected chrome (`presentation_hit/touch.rs:147-288`); deleting it changes input behavior.

**Avoidable amplification:** a surface rebuild is also a request to rediscover whether each entire derived view can be reused. An immutable generation can carry the views already linked to its records, and immutable subparts can be shared when their **actual** inputs are unchanged. A content-selection change does not necessarily change splitter geometry or every bar's pointer region. Conversely, selected minima or policy can change layout, so “selection only changes a selected bit” is not a valid assumption.

Do not implement a generic dependency-graph engine. First distinguish a bounded geometry/chrome result from content selection and concrete output binding, with a small explicit dependency list. Reuse whole immutable subparts only when those dependencies have been established at their owner. Existing topology and RecordOrder reuse remain useful and are already implemented; they are not the missing feature being claimed here.

### 4. Most paint records still return to semantic names to retrieve receiver rows

**Direct evidence, high confidence; current sampling supports relevance, exact share unqualified.**

The surface program already links typed record rows (`scene/program.rs:142-163`, `715-789`). Its runtime visitor resolves those linked rows directly (`scene/program.rs:793-909`). However, ordinary paint receiver helpers take that resolved record, recover its stable semantic ID, and look it up in the separate hit table again: panes/tabs at `runtime/paint.rs:2271-2301`, bars at 2306-2359, splitters at 2405-2421, and the common `receiver` at 2616-2626. Hit lookup uses binary search over semantic region IDs (`presentation_hit.rs:482-490`). Tab operability also rejoins the bar by ID (`runtime/paint.rs:2596-2613`).

The latest target change **already** binds DropTarget rows and centers once (`scene/paint_program.rs:28-68`), so repeating that recommendation for targets would be obsolete. Its narrow success is evidence for extending the private linkage to other ordinary receiver roles, not evidence that the rest of the linkage is complete.

**Necessary work:** emit current output-bound descriptors, retain absent/disabled/passive roles, and reject stale or foreign wrapper usage according to the existing API contract. A stable ID remains valuable for public events, serialization, diagnostics, and widget keys.

**Avoidable amplification:** repeatedly joining two views of one immutable record through that external ID on each paint traversal. Add optional role rows to the linked operation or record view and bind descriptors to the current small output wrapper. Several roles per tab/bar must remain distinct. A wrapper lifetime alone is not proof that two wrappers came from the same plan: callers can borrow two plans with the same lifetime. Use a private generation binding plus checked row access, not an unchecked row cast or weakened foreign-wrapper behavior.

## Required semantics versus representation choices

| Contract to retain | Work it really requires | Current representation is not mandatory |
| --- | --- | --- |
| Complete measurement answers, including unavailable answers | One answer for every current Q, value/variant checking, stable errors | BTreeMap answers followed by BTreeSet membership joins |
| Valid scene geometry | Check containment, clipping, coverage, and coherent measurements or establish these invariants during construction | Re-derive all structure, or trust every computed rectangle without checking |
| Pointer and touch behavior | Preserve all regions, touch fragments, lane/stack precedence and occlusion | Independent stable-ID joins between every consumer and producer |
| Keyboard/accessibility receivers | Preserve receivers even when pointer-clipped | A pointer-only receiver table |
| Delayed/discarded/stale output safety | Bind input authority to final presented emission and coordinate generation | Deep equality over the entire immutable scene on every publication decision |
| Independent UI adapters | Stable domain model and adapter-neutral geometry/operations | Every internal phase behaving like a separately untrusted adapter submission |

The existing consuming `CompiledSurfacePlan` already removes repeated structural recipes. Production `finalize_compiled` retains real geometry and relation checks (`scene/validation.rs:149-186`), not just redundant membership bookkeeping. The counterexamples documented in `2026-11-06-compiled-finalization.md` include a viewport outside its bar, a Visible tab with no drawable record, wrong contained-root/source geometry, and zero-content coverage under combined occlusions. These cannot be dismissed as unnecessary checks.

Likewise, output-generation identity cannot replace final-presented authority. New emissions of one shared generation still need independent emission keys and settlement. The scene identity and output receipt are different dimensions.

## Recommended module and data-flow design

The recommended design is a **private compiled-generation module with a row-bound measurement input and thin output bindings**. Keep the external contracts during migration.

1. `FrozenSurfaceRequirements` owns the immutable request rows in deterministic class/key order. It retains stable external keys and the full Q. Its internal compiler-facing API supplies typed row references, not arbitrary detached keys.
2. `MeasurementBatch` owns row-aligned value arrays bound to those requirements. A synchronous owner fills them in the core-issued loop, checking each answer variant/value and retaining stable unavailable/error ordering. A detached external contribution validates its keyed batch once and imports it into the same representation. New measurements must still be read and compared when the application callback has no revision/stability contract.
3. A consuming `CompiledSurfaceGeneration` owns the validated record arenas, the already-linked surface program, the pointer/touch view, the semantic view, and per-operation receiver-role links. Keep pointer and semantic masks independent. Retain public stable-ID views and current ordering contracts; if canonical ordering remaps rows, do it once at the owning finalization boundary, as current linking already does.
4. `SurfaceOutputBinding` holds the scene stamp/output ticket/coordinate capture and an `Arc` to that generation. Candidate and retained fallback bindings may reference different generations. Final presented emissions and interaction authority remain separate small capabilities. Rebinding an output to the same generation never needs to traverse record contents.
5. A published-scene comparison examines the mutable binding/header facts and exact generation identity. Preserve content equality as a separately named diagnostic or compatible fallback; do not globally redefine public `PartialEq` until its consumers are audited. Existing engine-local scene stamps are useful; do not add another content fingerprint. A generation `Arc` identity is sufficient to prove same-owner reuse without claiming that independently constructed equal values have the same identity.
6. After the identity/input seams are established and measured, allow explicit immutable geometry/chrome subparts to be retained across a local selection when their true dependencies match. Do not begin by building a universal incremental compiler. Selected minima, policy, contained projections, popup state and coordinate changes must still invalidate affected outputs.

The important simplification is ownership: within this module all views come from one private constructor and one frozen record arena. Membership and association need not be repeatedly rediscovered. Trust is not granted to external bytes, late receipts, application-owned mutable callbacks, or arbitrary geometry merely because it is Rust code. This extends the current consuming-plan boundary to the whole installed generation.

### Migration and risk

Start with the generation identity seam and ordinary receiver row links. They can preserve the current complete inventories and checks and do not require changing measurement APIs. Next pilot row-bound measurement internally behind the synchronous helper, leaving public keyed contributions intact. Consolidate generation ownership and only then consider geometry/content subpart reuse.

Costs include moving internal APIs, extra role-link storage, and maintaining a stable public view over private rows. The current latest target-link change already demonstrated that links have measurable retained-memory cost; extend it only with allocation/live-size accounting. The main correctness risks are rows surviving arena replacement, conflating semantic and pointer reachability, dropping mutable header changes from state equality, accidentally authorizing a fallback as current, and incorrect invalidation when minima/policy affect geometry. All of these are bounded module invariants; none calls for a new compiler-like verification script.

This design is worthwhile even if the numeric solver remains unchanged. It removes whole classes of association and comparison work. It does not promise ImGui parity or justify an immediate repository-wide rewrite before the experiments below.

## Alternative: preserve current documents and optimize their owner-controlled seams

A smaller alternative keeps `PresentationPlan`, hit/semantic manifests, and output wrappers as separate current types. Add explicit identity fast paths while preserving value-equality fallback; extend the current paint-program receiver links to ordinary roles; add a private row-filled measurement builder feeding the existing compiler; replace two-way tree membership probes for truly detached exact-set validation with an ordered merge if it remains important. Keep the current full derived-view builds on real surface changes.

This has lower migration cost and preserves current diagnostics and public data access. It is appropriate if changed-scene compilation is no longer a dominant end-to-end cost after idle identity and receiver work are removed, or if actual geometry/semantic changes make immutable subpart reuse uncommon. Its limitation is that the independent-document model remains: later contributors can accidentally reintroduce deep equality, rejoining and repeated view derivation. The compiled-generation module makes that invariant local and explicit.

I recommend the compiled-generation architecture as the destination, but the smaller alternative is a valid stopping point if measured wins flatten. The experiments should decide how far to migrate, not an assumed need to erase the entire product ratio.

## Minimal falsifiable experiments

No experiment below was executed by this lane. Keep formal CPU runs serial and separate from builds, sampling, active allocation instrumentation, and tests.

1. **Exact same-generation equality pilot.** Preserve existing equality results, add pointer-identity fast paths only for already shared immutable generation members, and instrument how many content comparisons they avoid. Compare complete 16/128/1,024-pane idle and click workloads and verify all scene deltas/published-state results are identical, including rollback/discard and header-only authority changes. If current shared-owner fast paths hardly trigger or the end-to-end improvement is within repeat noise, reject equality as a major optimization priority. If deep comparison disappears but latency barely moves, that directly limits how much the generation redesign can claim for idle.
2. **Full-Q row-bound measurement pilot.** Feed the exact same resolver answers and callbacks through current keyed batches and a private row-aligned batch. Keep every required item minimum, intrinsic, strip, popup and bounds answer. Exercise unavailable answers, wrong variants, ticket changes, minima/policy changes and stable error order; detached malformed keyed batches must still be rejected. Count allocations, membership/lookups and retained bytes, then time the complete unchanged workload. If joins were small compared with actual text/resource callbacks, reduce this migration's priority. A faster microbenchmark alone is insufficient.
3. **Linked-view/selection pilot.** First link ordinary paint receiver roles without skipping any records or input checks; compare descriptor/program output against the current path, including absent/passive and foreign wrappers. Separately instrument full derived-pass visits for selection under equal minima versus selection under changed minima/policy, plus resize, popup clipping and combined occlusion cases. A two-part generation pilot must match hit/touch fragments, semantic receivers including clipped rows, program order, output binding, stale fallback and final-presentation behavior. If most selection changes actually alter the relevant geometry dependencies, or extra memory outweighs end-to-end gains, reject fine-grained generation reuse and retain the smaller alternative.

These experiments can disprove the recommendation's priority without disputing the source facts. A large renderer/output-audit remainder after invariant work is removed is also a reason to stop this module's optimization, rather than deleting guarantees to chase a whole-product ratio.

## Lower bounds and limits of the claim

Retaining the exact current synchronous workload gives a real callback-count bound: without a stability promise, the owner must obtain all Q requested values. It also must inspect enough values to determine whether arbitrary callback answers changed. Row association removes tree work but does not let the core skip application callbacks. This is a contract/workload bound, not a universal docking bound; an explicit application revision contract could change it later.

A changed result must represent all records/fragments that its contract exposes. If an output has F distinct touch fragments, emitting those F fragments costs work proportional to F; fragment count depends on actual geometry and overlap. Full painting of all visible contents likewise retains its actual callback and output work. These facts do not require re-solving unchanged geometry, comparing known identical scene facts, or rebuilding every receiver view after every local mutation.

There is no evidence here for a millisecond floor, an unavoidable 6x/12x/15x language-level penalty, or a lower bound equal to current headless timings. Numeric solver samples are small; whole-pipeline costs and adapter work are separate. Framework neutrality, ownership, and correctness justify explicit boundaries, but they do not inherently require multiple self-validating documents inside one closed immutable owner.

## Verification sources and exclusions

The review checked current source at the references above against the latest `2026-11-06-linked-targets.md`, `2026-11-06-title-resources.md`, and `2026-11-06-compiled-finalization.md`. Shared manifests, restricted snapshot reuse, exact structure reuse, RecordOrder reuse, consuming finalization, same-pass title resources, demand-driven focus lookup, and DropTarget receiver installation binding are already implemented and are not reported as missing.

Fresh sample sources supplied by root are `current-idle.sample.txt`, `current-idle-attribution.json`, and `current-tab_click-attribution.json` in this task's temporary directory. This lane did not independently reproduce them. Numerical savings, complete semantic equivalence of a proposed generation split, and current ImGui kernel comparability remain unproven.

## Counter-review: a generation container is not an optimization

This addendum sharpens and partly narrows the original recommendation. The current `SurfacePlanScene` already owns the plan, hit manifest, paint link, semantic manifest, and coordinate capture (`scene/lifecycle.rs:117-126`). Putting those same fields in another `CompiledSurfaceGeneration` and adding one more Arc does not eliminate a traversal, lookup, comparison, allocation, or authority check. It can increase indirection and retained memory. **The current recommendation is to stop at bounded protocol changes unless a selective-reconstruction prototype earns the broader split.** A full generation rewrite is a conditional option, not a prerequisite or a justified default destination.

The three candidate boundaries below are the observable changes that would have to make the architecture substantive. Their names and top-level container are incidental.

### Three boundaries that must actually change

| Boundary | Observable before/after | Does it require a new container? | What would prove the change is only cosmetic? |
| --- | --- | --- | --- |
| Record-to-receiver association | Today a resolved pane/tab/bar/splitter record is converted back into a semantic name and binary-searched in the hit inventory during paint. The revised execution path consumes a role-specific optional receiver row already associated with that immutable record, checks its binding, and attaches the current output ticket. | No. Extend the current `SurfacePaintProgramLink` and private paint views; existing `SurfacePlanScene` can own it. | The new wrapper still calls `receiver(kind)`/`region_for_kind` per ordinary painted role, or keeps both the new link and a mandatory old lookup. |
| Surface reconstruction after local state change | Today a Ready miss invokes whole-surface compilation/finalization and then separate hit/touch/semantic installation passes. A revised path retains explicitly unchanged immutable geometry/chrome and its linked views, while rebuilding only affected content/semantic/output parts. | A genuine split of dependency and ownership units is needed; a new top-level name is not. It could be expressed inside the current `SurfacePlanScene`. | Selection still rebuilds every record/view and merely compares the new result to the old one, or a new dependency fingerprint costs another complete equivalent pass. |
| Synchronous request-to-answer relation | Today the core-issued loop serializes answers into keyed maps, followed by exact-set joins and keyed compiler access. A revised synchronous path carries its complete row association into actual compiler and retained-measurement consumption, with keyed submissions imported only at the detached seam. | No generation container is needed. A bounded measurement-input change is needed, and it reaches beyond the request loop. | The row batch is converted back into the same maps before compilation or retained alongside all old maps; the claim then amounts to moving or duplicating work. |

Only the second boundary warrants talking about a substantial generation decomposition. The first can deliver its benefit without it. The third is an independent storage/input problem. Scene identity fast paths are another independent small fix and do not establish that any of these boundaries changed.

Even a single module with a private constructor does not automatically make every association constant time. The relation must actually be retained and used by consumers. Conversely, the existing private fields and consuming finalizer already provide much of the ownership discipline; they should be extended rather than surrounded by a second proof protocol.

### Benefit ordering, separated from confidence

The following ordering is a forecast of end-to-end leverage for the existing workload suite, not a measurement or a ranked list of easy implementations. There is no supplied workload-frequency distribution that can collapse idle, two-frame click, drag, and resize into one honest scalar. The ordering consequently states its case dependence.

1. **Ordinary receiver-role linkage has the best currently grounded recurring benefit.** It affects every painted frame, including idle and both click frames. Fresh idle receiver queries account for an inclusive 7.722% sample category, while the already completed narrow DropTarget installation linkage produced qualified approximately 3.2% full-product idle improvement. These facts support modest recurring end-to-end potential; they do not predict another 3.2% or 7.722% saving for other roles. Some role work remains necessary. This is the first useful protocol pilot, rather than a reason to wrap the existing scene in a new owner.
2. **Selective reconstruction has the largest conditional click benefit, but its current expected benefit is not established.** Click compiler/hit/touch/validator samples show that avoiding genuine reconstruction could matter, while idle has no standalone scene compilation to remove. Those overlapping categories cannot be added into an achievable saving. If equal-minimum selection retains real geometry dependencies often enough, this option can outrank receiver linkage for clicks. If selection commonly changes the dependency inputs or stale-authority source facts, it may provide little sharing. A local pilot must establish that condition before a broad migration. It ranks below recurring receiver linkage for the current investment decision because the supplied evidence does not yet establish how much work can actually be avoided, not because its implementation is harder.
3. **Equality identity fast paths are a modest recurring opportunity, not the principal remaining bottleneck.** Root's broader fresh idle `SurfaceSceneSet::eq` subtree contains 271/6,320 samples, or **4.288% inclusive**. The narrower standalone `HitGeometry::eq` 1.139% and layout-facts equality 0.411% are nested/partial observations, not the complete equality budget and not numbers to add to 4.288%. None can be converted into exact frame milliseconds. This is more relevant than a claim of only about 1% total equality, but remains insufficient to explain the large product gap. It provisionally ranks above full-Q association because its recurring work is visible while the removable association share remains unisolated; that ordering can reverse after measurement. Its high implementation confidence is not a reason to launch a generation rewrite.
4. **Full-Q row batches have lower, currently unisolated expected benefit.** The 11.472% idle measurement/resource category includes real title/resource callbacks, not just map assembly. All those callbacks and their values remain. Retained future-layout projection also still needs semantic-key lookup. There is no evidence that the removable association part is most of this category or that migrating all consumers will beat a simpler storage change. Do not treat the entire measurement sample as a row-batch opportunity.

For idle specifically, selective reconstruction offers no demonstrated benefit; recurring receiver work is the primary opportunity among this lane's proposals. For click specifically, selective reconstruction could become first after the dependency-locality pilot. Output audit, framework widget work, real callbacks and tessellation remain outside this lane's proposed savings and may dominate the end-to-end remainder.

### Full-Q row batches: the compatibility costs are real

Frozen request order makes filling arrays feasible. It does not make the entire measurement protocol positional. There are five constraints that the original proposal understated:

1. **Callback order and error timing are observable.** The helper requests dock bounds, optional popup bounds, all pane minima, all intrinsics, and all strips in that order (`runtime/measurement.rs:217-282`). Wrong variants and invalid intrinsic widths fail immediately (`runtime/measurement.rs:116-156`), so callbacks after such an error are not currently executed. An `Unavailable` answer is stored and does **not** stop the loop. A later wrong variant can therefore win over an earlier unavailable answer. An eager row builder that returns on the first unavailable value changes both callback side effects and final error. Successful or merely unavailable batches must still execute the complete Q; it is not correct to say every invalid batch always executes all Q.
2. **Exact-set and unavailable error order are distinct phases.** Detached preparation checks stale base, ticket, policy and coordinates first (`engine/surface_contribution.rs:255-283`). Manifest validation then checks the ticket's fields in a defined order (`scene_manifest.rs:1402-1441`), dock/popup presence and identities, then pane/intrinsic/strip sets (`1067-1103`). Within each class it reports the smallest missing key before any unexpected key, even when an unexpected key sorts earlier (`1445-1458`). Only afterwards does `require_authoritative` report unavailable answers in bounds/popup/class/key order (`1295-1334`). A naive sorted zip that reports its first mismatch, or a conversion that reports unavailable data before missing keys, changes this behavior. A merge implementation can preserve it, but must deliberately preserve these priorities rather than being described as a free simplification.
3. **The detached API is genuinely independent.** `SurfaceMeasurements::new`, setters, duplicate-detecting insertions, `SceneRequirementManifest::validate_surface`, and `DockEngine::prepare_surface_contribution` are public (`scene_manifest.rs:940-951`, `1133-1143`, `1190-1279`; `engine/surface_contribution.rs:244-248`). Callers can submit partial, stale, extra or unavailable batches. Replacing this interface with an externally positional array tied to an inferred ordering would weaken the boundary or break users. A synchronous-only path can be additive, but then there are two input representations to maintain until normalization genuinely replaces one.
4. **Compiler consumption is not in request order.** Requests use key order (`scene_manifest.rs:1028-1043`); `compile_tab_strip` consumes workspace item order and builds keys for each item (`scene_compiler/surface.rs:1254-1261`). Root measurement filters requests per root and materializes retained root minima (`2007-2021`). Mere array filling does not remove these lookups. It requires an actual key-to-row directory, linked requirement handles, or a refactor of the consuming loops. If all consumers still binary-search a key, the proposal is a contiguous-storage optimization rather than elimination of association. That may still be useful, but is a smaller claim.
5. **Retained measurements are used beyond the original generation.** `PresentationLayoutFacts` retains the complete batch and exposes keyed intrinsic/strip lookup (`scene/records/layout.rs:35-80`). Future drop layout tries candidate keys and source keys across target and source facts (`scene_compiler/future_layout.rs:390-431`); pane minima have the same source-fallback behavior (`473-500`). A row valid only for the original request table cannot replace these semantic queries. Keys or an index must remain available, with explicit rebinding across source/candidate locations. Native first-live compilation also currently consumes the same `SurfaceMeasurements` path (`engine/surface_contribution.rs:346-360`).

The viable design space is therefore narrower than “replace maps with three Vecs.” A row-bound batch can retain the immutable request directory and use three value arrays, preserving keys through that directory. Detached keyed input can normalize into it after the existing checks. However, if this entails a new input abstraction across the compiler, Ready matching, retained layout facts, future projection and native compilation, its migration and compatibility cost may exceed its removable runtime cost.

The lower-cost challengers should be considered first: preserve the current API and error phase boundaries, use existing ordered inventories more directly, or pilot contiguous keyed storage behind current lookup accessors. An ordered exact-set merge must preserve missing-before-unexpected priority. Neither a storage pilot nor a private complete-batch constructor needs a new generation owner. Do not add provider-routing abstractions or a generalized measurement proof layer simply to accommodate both representations.

**Current row-batch decision:** defer a complete compiler-facing migration until a counterfactual experiment isolates material cost in assembly/set membership/keyed readback after keeping all callbacks. If the experiment merely saves duplicate-detection instructions while preserving most tree/index work, retain the simpler current protocol or a narrow storage improvement.

### Explicit stop/proceed criteria

These are proposed engineering decision gates, not measured results or universal thresholds. Their purpose is to prevent an architecture migration from being justified by an impressive but irrelevant microbenchmark.

- **Proceed now with bounded changes:** ordinary role linkage if it preserves all descriptor and foreign-wrapper behavior; a compatible immutable identity fast path if it stays local. Keep their performance claims separate. Neither licenses a broad generation split.
- **Stop at the bounded design** if selective-reconstruction instrumentation shows few unchanged geometry/chrome inputs, if the pilot still needs complete re-emission to discover reuse, or if the remaining cost is mainly actual callbacks/audit/framework work. Also stop if generation-only benefits cannot be reproduced in complete frame timing after the narrow changes. A new container with identical work is rejected regardless of naming or code tidiness.
- **Proceed with a geometry/content/output decomposition only after a local prototype actually skips at least one whole derived construction pass** for the ordinary equal-minimum selection case, preserves the necessary changed-minimum/policy and resize fallbacks, and shows a material complete-workload benefit. For the size of this migration, my proposed gate is at least **10% improvement in the 1,024-pane complete two-frame click p50 in each of two independent serial comparison groups**, measured against a frozen baseline that already includes accepted narrow changes, with renderer-free selection improving in the same direction. This is a qualification target, not a forecast of achievable speedup. A smaller repeatable gain can justify the narrow pilot without justifying the full migration.
- The same prototype must preserve the full Q, output/program counts, pointer/touch/semantic behavior, stale/final-presentation boundaries and rollback semantics. It must show no repeatable regression greater than 3% at 16/128 panes or other ordinary idle/resize cases in those groups, and no retained current-plus-fallback memory increase above 10% without a specific accepted tradeoff. These guardrails are proposed budgets; current evidence does not establish that the design can meet them.
- **Do not use the equality pilot's result as the admission test for decomposition.** Even eliminating every sampled equality instruction does not demonstrate that partial scene reconstruction is possible or that the wrapper structure is the dominant product cost. If a large remaining gap survives all these bounded opportunities, the correct next investigation can be in another module rather than a more elaborate generation system.

The decisive artifact for a broader refactor is therefore an avoided-work trace plus complete-workload timing and semantic equivalence, not an architecture diagram showing one owner. The current owner already co-locates the views. Only a changed association, reconstruction, or input protocol earns a performance claim.
