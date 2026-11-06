# Owner-directed presentation with three entry points

Design date: 2026-11-06. Read-only source review at documentation HEAD `34ab23a`; reviewed runtime base `5484071`. No build, test, benchmark, network operation, or repository mutation was performed. This is an executable design proposal, not a measured result.

## Decision

Make the existing engine/frame owner the Module. Give callers one transactional frame, one surface presentation operation, and one publication operation. Workspace edits, measurement association, compilation, and interaction authorization become its Implementation. Do not introduce another scene generation owner around `SurfacePlanScene`.

The important change is that the editing owner knows which leaf it changed. It updates that leaf's display facts and a small command authorization record. It does not rediscover the edit by capturing the complete root, deriving a manifest, rebuilding all records, or comparing a freshly rebuilt scene with the old scene.

The destination permits breaking changes. Remove the detached measurement/contribution protocol and public mutable presentation plans. Replace historical value fingerprints with owner-issued, conservative command authorization. Preserve selection/MRU, actual minima and effective policy, reveal/scroll, rollback, native lifecycle, stale/unpresented-output rejection, and the complete measurement and paint work.

## Interface and real Seam

The Module is the existing `DockEngine` plus its borrowed frame. These are its three entry points; construction/configuration and read-only inspection are ordinary setup, not an additional presentation protocol:

```rust
impl DockEngine {
    pub fn begin(&mut self, input: FrameInput) -> Result<DockFrame<'_>, Error>;
}

impl DockFrame<'_> {
    pub fn present<A: SurfaceAdapter>(
        &mut self,
        surface: SurfaceId,
        adapter: &mut A,
    ) -> Result<EncodedSurface, Error>;

    pub fn finish(self, completion: FrameCompletion) -> Result<FrameReport, Error>;
}

pub trait SurfaceAdapter {
    fn measure(&mut self, request: MeasurementRequest) -> MeasurementAnswer;
    fn paint(&mut self, output: PaintView<'_>) -> Result<PaintEvidence, AdapterError>;
}
```

The code is an Interface sketch. `FrameInput` contains the existing ordered ingress, presentation observations, current host/coordinate facts, and requested policy/configuration updates; it does not expose compiler revisions or requirement inventories. `FrameCompletion` contains adapter completion facts after final hooks, including failure/repeat-pass information. It is not permission to assert that arbitrary incomplete paint is complete. The current output auditing and native observation mechanisms supply these facts until an equivalent replacement is implemented.

The Adapter Seam is measurement and painting: it varies today between egui and the owning headless recorder. Workspace, layout, and compiler are concrete in-process dependencies. They do not need traits, provider selection, a protocol bus, or a second source of truth. egui continues to own widget registration, content child UIs, font shaping, final hooks, and native blockers. The recorder owns the complete ordered command/geometry recording. Both consume the same `PaintView`.

```rust
let mut frame = engine.begin(input)?;
for surface in surfaces {
    let mut adapter = EguiSurfaceAdapter::new(ui_for(surface), panes, style);
    frame.present(surface, &mut adapter)?;
}
let report = frame.finish(egui_completion)?;
```

For the headless Adapter, change only `EguiSurfaceAdapter` and completion evidence to the recorder's equivalents. Native callbacks enqueue observations for the next frame through the existing lifecycle ingress. Paint-discovered actions are collected inside the frame using the current action timing; this proposal does not require moving close, focus, or widget-discovered input to the next application frame. Repeat passes stay in the same attempt and reuse its frozen previous-pass facts. A dropped frame publishes no candidate state.

`PaintView` exposes the current ordered program traversal and receiver-bound operation helpers. It does not expose setters, raw table row indices, measurement tickets, or a separate `finalize` call. A caller can render everything by crossing one Seam and cannot accidentally forget compiler validation, hit linking, or requirement reconciliation. This is the Depth of the Interface. Existing host-frame calls may be kept while migrating internal callers, but the destination deletes their redundant alternative protocol rather than keeping compatibility wrappers indefinitely.

## Ownership before storage

The owner has four kinds of facts, with distinct lifetimes:

| Fact | Owner and representation | What changes it |
| --- | --- | --- |
| Validated workspace structure | Engine-owned topology and membership; existing stable node/item identities | Structural edit, resize weights, root ownership, restore |
| Current mutable workspace intent | Selected item and exact MRU per tabs leaf; transient strip state | Selection, close/reorder, reveal, scroll, menu/gesture |
| Reusable presentation facts | Immutable topology-corresponding fact tree, stable metric slots, solved geometry and private row links | Their actual input changes |
| Current command/output authority | Small owner records and emission ledger, separate from facts | Relevant semantic edit, policy/coordinate/native lifecycle change, new emission, observation/retirement |

For the first implementation, retain the current workspace clone for general structural transactions. Selection does not require an arena rewrite: add an owned leaf edit in the frame draft, containing the selected value and MRU replacement for only that leaf. The current root topology remains borrowed/shared. Commit installs this replacement atomically; discard drops it. Structural operations materialize a conventional candidate and retain the existing validation path. Read access must use one owner-controlled workspace view that consults the leaf edit before the base workspace; do not leave ad hoc direct reads that see the old selected value.

This overlay is a transaction implementation, not a second public model. Later, moving selected/MRU out of `Node::Tabs` is a natural simplification, but it is not a prerequisite for proving that presentation construction became local. Do not combine this work with a persistent-arena rewrite.

## Reusable facts: a tree, not another generation container

Replace the flat all-fields `PresentationPlan` as the internal storage model. Each tabs leaf has one immutable block of rows and links. Split nodes have the child block references and split facts. A surface retains its root references and separate surface-wide/overlay facts.

```text
SurfaceFacts
  roots -> FactNode tree(s)
  surface/overlay facts
  measurement directory

FactNode::Split
  split geometry + receiver/program links
  children: Box<[Arc<FactNode>]>

FactNode::Tabs
  geometry: Arc<LeafGeometry>
  display: Arc<LeafDisplay>
  receiver/program/semantic recipes using private local rows
```

`LeafGeometry` includes pane/content rectangles, tab/chrome rectangles, clipping and pointer geometry. `LeafDisplay` includes the selected item and the selected tab flags, plus title/menu facts that depend on selection. A row's command recipe identifies the logical role and item/node; it does not capture an entire `ItemSource` or `NodeFingerprint`.

Patching a leaf path copies the changed leaf and ancestors, sharing unaffected children. Full rendering walks this tree in canonical order. There is no flattening pass into a newly allocated surface-wide `Vec` and no new surface-wide row linking pass. The existing linked program establishes the intended order; its replacement traverses prelinked per-block instructions with root/overlay begin/end events.

The current `WorkspaceIndex` cannot simply be retained unchanged under a new workspace version: its selected-rule/source-fingerprint facts are not structural facts. Extract its root/node/item ownership directory into the topology owner and read selected rules from the draft leaf view. Replace its global-version validity guard for these private structural reads with ownership by that immutable topology. Any remaining public/frozen-reference use keeps live validation until migrated; never bypass its version check merely because this edit was Select.

The complexity is `O(changed leaf rows + sum of child-array sizes on the ancestor path)`, in addition to required full measurement and painting. Do not claim constant time or logarithmic time for arbitrary workspace trees: a skewed tree or a very wide split can make this bound large. The bounded initial implementation is a plain `Arc` tree using existing dependencies. If measured path-copy cost later matters, changing child storage is private Implementation work.

An `Arc<Vec<Arc<LeafBlock>>>` is insufficient: cloning its top-level vector on every selection still touches every leaf. A patch map over a vector is also insufficient if installation immediately flattens it. A parent-path copy avoids both. Native/contained/global overlay records stay separate and can use the full conservative constructor until their dependencies are made explicit.

## Measurement ownership

Topology construction assigns private slots to all relevant pane minima, tab intrinsics, and strip measurements. The directory preserves semantic identity and request order once. `present` issues every current measurement question and stores the answer directly into its owned slot. A typed answer check validates the scalar/variant as it is received. No public key is returned and resubmitted to the core, no `BTreeMap::insert` is needed for each answer, and no second exact-set traversal checks membership that the owner just enforced by construction.

The current Q remains complete: every item minimum required for future drop projection, each required intrinsic, each strip, and both relevant bounds. No metadata revision permits skipping callbacks. While measuring, compare each answer with its previous slot and record the affected local dependency. This comparison is useful once; it replaces whole-measurement-map equality and later repeated joins. It does not remove the `O(Q)` required callback/read work.

Effective tab-bar policy is computed before its metric questions from the current leaf's selected rule. A selected item's rule can change effective policy even when the policy snapshot revision is unchanged. If a local policy change adds/removes tab/strip questions, construct only that leaf's current request schedule. Continue all requests after an explicit unavailable answer and fail the contribution once the complete required loop has finished; invalid answer variants/scalars can fail immediately. Breaking the detached protocol also removes its arbitrary missing/unexpected-key error precedence, which is no longer part of this Interface.

Preserve origin for future-layout queries. A minimum is measured for `(source leaf, item)` and may depend on that leaf's visual identity; it must not silently become an item-global minimum. The stable directory maps source identities to slots, and an internal candidate/drop view carries the source slot when simulating a move. A destination position never indexes an old array using a new ordinal. Rebuild the directory on topology/membership change, retaining old directories with old emitted facts while referenced.

## A single ordinary Select, step by step

1. The editing owner validates the current action or presentation grant, item membership, and policy. The existing selected/MRU update yields a private leaf edit with the exact before and after selected/MRU values. It does not clone/scan the full root to prove its own field assignment.
2. The owner recomputes that leaf's effective tab-bar policy from the new selection. Requirement membership for all unrelated leaves is retained without visiting those leaves. The semantic index's stable topology and membership do not change.
3. `present` still asks the complete Q. It updates slot values and records which leaf metrics or surface bounds changed. Measurements can change for unrelated leaves in the same frame; those leaves must enter the affected set even though Select caused only one edit.
4. Compute the changed leaf's actual effective outer minimum from its newly selected minimum, config and tab-bar visibility. If all effective minima, bounds, split inputs, and coordinate facts are unchanged, reuse the solved root geometry. Equal raw content minima alone are insufficient.
5. Recompute that leaf's display, selected reveal, clamp, chrome and semantic state. If the reveal/scroll result and geometry inputs are unchanged, retain its `LeafGeometry` too. Only the leaf display/semantic block and ancestor path are replaced. Unrelated leaf records, program links, hit rows, and geometry are never rebuilt or compared.
6. If a minimum changes, rerun numerical layout for the affected root using the same compiled topology. Any leaf whose resulting rectangle changes gets new geometry. This is a real nonlocal dependency, not an incremental miss to conceal. Changed strip metrics or scroll update the affected bar; policies/overlays may require conservative surface work.
7. Associate the reused facts with current semantic authorization and a new output emission. Stream the complete program through the Adapter and retain final output evidence. Only actually observed, complete output can authorize subsequent presented input.

A full measurement loop is permitted to find unrelated changing metrics; a full structural or scene traversal solely to discover that Select did not change them is not. Both are currently mixed in the manifest/compiler path.

### Touch and global spatial facts

Do not assume local pointer-row changes imply local touch-fragment changes. Protected chrome, neighbors, occlusions and magnetic/drop coverage have nonlocal dependencies. In the first slice, geometrically unchanged selection reuses the exact global hit/touch geometry by construction: no hit row changed, so no re-emission/equality pass is necessary. When reveal scroll changes receiver geometry, use the existing global derivation where needed until the dependency is correctly represented. It is acceptable for this branch to rebuild a global touch index; it is not acceptable to label that branch local without counting it.

The fact tree supports local pointer traversal or a later index, but it does not justify deleting global geometry validation. Combined contained occlusion, out-of-bar viewport, missing visible-tab records, and lost drop coverage are known real counterexamples that remain meaningful.

## Identity, authorization, and failure semantics

Geometry sharing never grants authority. The owner maintains the existing authority domain, host/binding, coordinate capture, emitted-output identity, and observation ledger. An emission points at the immutable fact root and its command basis; it contains no clone of the entire workspace.

Replace root value fingerprints used as internal command proof with a small per-root command revision issued by the editing owner. Relevant membership, selection/MRU, ownership, or policy changes advance/revoke that command basis. Command execution checks the grant's emission against retained, actually presented output and checks that the relevant root basis remains current. It then reads current membership/rules from the owner. Root revision changes do not invalidate geometry or request inventories. The revision exists only for command admission; the edit itself drives fact updates.

This intentionally permits conservative rejection after an intervening edit, even if a root's values later return to the same values. Preserving value-based ABA acceptance is not required. Retaining an immutable command basis or checked monotonic serial prevents an old grant from accidentally regaining authority. Exhaustion is an explicit error; rollback must never recycle externally observable output IDs or command grants.

Candidate facts, paint fallback, actually emitted output and observed interaction authority are separate states. An unobserved candidate cannot authorize commands. Retained fallback may still paint; it does not automatically inherit the new candidate's command revision. A stale/revoked emission cannot authorize a fresh press. An already admitted drag/press can continue through an owner-held capture/session whose lifecycle is independently validated; do not re-admit later edges from an old output after a selection made the initiating grant stale. This keeps ordinary press/release behavior without retaining root-value proofs.

Shared semantic blocks contain recipes and presentation facts, not pre-authorized `ItemSource` values or output tickets. Bind their receiver views to the current emission while traversing or querying. This is essential: leaving a source fingerprint or output ticket in every shared semantic row would force reconstruction of all those rows and defeat the split.

Native first-live staging remains a distinct owner state: a projection has source ownership, saga phase and binding provenance. Source edits, recovery, failed creates, rehome and retirement revoke it through the same existing lifecycle owner before emission. Retain those lifecycle checks while changing fingerprint representation. Never let shared geometry carry a native admission grant.

Rollback drops the leaf edit, new facts and uncommitted ledger changes together. Output allocation stays non-rollbackable as today. Adapter side effects cannot be rolled back, so discarded encoded output is never published as interactive. Final hook failure invalidates the relevant paint evidence even if facts and command recipes are valid.

## Change detection without repeated value equality

The publication record consists of immutable fact-root identities plus explicit small semantic/lifecycle headers. Equality of an unchanged shared root is constant time. `published_state_changed` compares the initial and final publication records, including candidate/fallback, observed authority, coordinates, native provenance, popup, and revision frontiers. It must not be an OR of intermediate changed flags.

Canonical no-op decisions belong to their owner. Reselecting the current item is a no-op when MRU is already valid; an A-to-B-to-A selection sequence may still change MRU or revoke grants and therefore can legitimately change published state. If every observable state is restored, reuse the initial immutable root/header rather than returning a freshly allocated equivalent and reporting a false change. Broad structural transactions can retain final semantic comparison until they have an equivalent narrow construction rule. Remove hot-path deep equality incrementally; do not weaken published-state meaning merely to remove a trait implementation.

## Precise migration and deletion map

The first column names current source, not hypothetical files. Line references are from this read-only review and may move.

| Current source | Replacement and concrete deletion |
| --- | --- |
| `engine/presentation_authority.rs:16,154,233,418,441,646` | Keep one presentation owner. Add an owner-private local-edit path. For this path delete complete `SurfaceSemanticSnapshot` capture, root fingerprint comparison, draft derivation, semantic-index reconstruction and strip roster reconciliation. Ultimately replace the broad snapshot-based change detector with edits from its actual owners; preserve the native lifecycle reconciliation responsibility. |
| `operation.rs:73-101,1456` | Return a private `SelectedLeafEdit` from validated selection and use a leaf draft. Remove `clone_workspace_draft` for this path, duplicate source recapture, and root-wide proof construction. Retain general structural transaction validation. |
| `transaction.rs:78-100` | Publish a known local selection result without whole-workspace `PartialEq`. Keep initial/final comparison where general batches still require it. |
| `graph.rs:166,357` and engine workspace access | Expose a single draft view over the owned selected/MRU edit. In the final cleanup split mutable selection/MRU from stable topology; migrate serialization/builders together instead of preserving duplicate selected state. |
| `workspace.rs:315,570,743` and `command.rs:42-195` | Replace internal fingerprint-based source capture with current owner lookup plus emission/root command basis. Once all engine/native callers migrate, delete `NodeFingerprint`, `RootFingerprintData`, `FingerprintNode`, comparison helpers, and source-held full root snapshots. Public low-level frozen-source command constructors can be removed in this breaking change. Do not remove native source-freeze checks; replace their representation. |
| `scene_compiler/requirements.rs:30-217` | Turn complete derivation into topology construction plus per-leaf policy/question scheduling. Remove re-derivation on selection and use direct local rows instead of `capture_tab_target` to retrieve a selected rule. |
| `scene_manifest.rs:657-1398` | Replace stable requirement sets/maps with private slots and a once-built origin directory. Remove public `SurfaceMeasurementTicket`, detached `SurfaceMeasurements` setters/maps, `SurfaceRequirements::validate`, and `AuthoritativeSurfaceMeasurements` exact-set wrapper after callers migrate. Keep scalar/variant validation and explicit unavailable answers. |
| `runtime/measurement.rs:201-297` | Call Adapter requests directly into owned slots and then the internal present operation. Delete map assembly, contribution-token creation and the resubmission round trip. Uniform metrics becomes an Adapter convenience, not a second data protocol. |
| `engine/surface_contribution.rs:230-395` | Replace `begin/prepare/compile` capability protocol with the owner's synchronous present Implementation. Delete ticket/base/coordinate/policy rechecks between private sequential calls; check current native/coordinate/policy admission once at the real external entry. Keep those checks for actual asynchronous ingress until that ingress is migrated. |
| `scene.rs:209-249` and `scene/records/layout.rs:36-118` | Replace flat `PresentationPlan` vectors and measurement-ticket provenance with fact blocks and slot-backed retained metrics. Remove cloned `SurfaceMeasurements` snapshots and duplicated per-root keyed minima maps. Keep origin-sensitive future-layout access. |
| `scene_compiler/surface.rs:1220-1370,1990-2065` | Extract leaf display/chrome/reveal construction and selection-dependent effective minima from the whole-surface compiler. Build one leaf block or one affected root as needed. Never rebuild every row and compare afterward. |
| `scene/validation.rs:41,152,594,1050,1104,1634` | Checked topology/block constructors establish roster/link uniqueness once. Validate changed block scalar/geometry facts on construction and global geometry on affected geometry changes. Move independent full roster reconstruction to a reference checker/test path; do not merely disable geometry/coverage checks. |
| `scene/program.rs:149-247,715-793`; `scene/paint_program.rs:10-68` | Keep typed private row concepts inside blocks and include optional role-specific receiver rows at construction. Replace global row-lookup linking with canonical block traversal. Delete `SurfacePaintProgramLink` and semantic-ID receiver re-joins once no caller needs them. |
| `scene/lifecycle.rs:119-128,1217-1305` | `SurfacePlanScene` stops owning geometry plus ticket-authorized manifests. Store fact-root references and small emission/lifecycle headers. Installation attaches current authority; it must not compile hit/paint/semantic views or compare entire plans. Retain candidate/fallback/observed-output state transitions. |
| `presentation_hit.rs`, `presentation_hit/geometry_reuse.rs`, `semantic_manifest.rs` | Replace rebuild-then-compare with block reuse and direct bound recipes. Keep distinct pointer, touch and semantic reachability. Remove full equality scans used to recognize already identical geometry. Retain/rebuild global spatial derivatives when their actual inputs changed. |
| `runtime/paint.rs:2271,2405,2616` | Receiver helpers use local bound row links and the current emission/call context; delete searches by extracted semantic ID. Keep paint completeness/final-hook evidence and foreign/passive-role rejection. |
| `engine/reducer.rs:938` and presentation `PartialEq` implementations | Compare publication roots plus complete small headers. Remove deep scene-value comparison from the hot path. Preserve initial/final state semantics and non-rollbackable allocation frontiers. |
| `egui_dockspace/src/product_render/measurement.rs`; `product_render/mod.rs`; `pass_settlement.rs`; `integration/dockspace-host-harness/examples/frame_cost.rs:172` | Implement the two real Adapters directly. Keep widget/child-UI/full-recording work and existing settlement. Delete old contribution-protocol plumbing after both cross the new Seam. |

The table describes a destination and an ordered deletion set, not permission to delete all listed types before their native/drag/drop consumers have replacement semantics. Build no aliases for old concepts solely to preserve names. Every intermediate migration should have one authoritative representation of a fact.

## Shortest executable slice

Do this before the broad public Interface replacement. The existing `DockspaceHostFrame` can call the private Implementation during migration.

1. Add one private `SelectedLeafEdit` result to the current validated ordinary tiled selection path. Keep the general structural path. Route it to a presentation-local path that retains the stable requirement/index inventory and updates only the effective policy of that bar. Initially retain workspace transaction cloning if changing its reads would enlarge the patch; this does not weaken the presentation construction claim.
2. Split one no-popup tiled surface's internal program/records into leaf blocks plus unchanged split/surface blocks. Implement full canonical traversal over blocks for both existing paint Adapters. Build this representation once from the conservative compiler; do not reconstruct blocks by slicing a newly compiled plan every frame.
3. Initially support the exact branch in which the complete new measurements match, effective policy matches, actual effective minima match, and selected reveal yields the same scroll and receiver geometry. Replace only selected pane/tab display and semantic facts, mint current command/output authority, and share every other block. No complete manifest/compiler/validator/hit/link/semantic pass runs for that selection. Use the existing conservative path on a failed condition, including changed minima, changed policy, new scroll geometry, popup, active gestures or native staging. The guard uses existing measurement results and local values, not a complete newly built scene.
4. Add counters at the actual current construction sites. At 16/128/1,024 leaves, prove the same number of measurement callbacks and paint steps, zero unrelated requirement/record/link constructions on the eligible Select, and changed leaf construction independent of unrelated pane count. Ancestor path copies are counted separately. This is the architectural acceptance test.
5. Then migrate slot-backed measurement and direct receiver links. Extend to changed local scroll/policy only after their hit/touch/global dependencies are tested. Then change the external Interface and remove the detached types. This sequencing avoids an all-at-once host/native rewrite before the principal avoided work is demonstrated.

The smallest first patch can retain full workspace copying, so it is not a complete solution to all click overhead. Its specific result must be real: a successful ordinary Select bypasses unrelated presentation derivation. Rebuilding all flat vectors and reporting pointer sharing afterward fails the slice.

## Counterexamples and tests through the Interface

| Counterexample | Required test and observable result |
| --- | --- |
| Equal content minima but new selection hides its bar | Effective policy/outer minimum changes. Full output matches conservative construction; geometry is not reused based solely on raw minimum equality. |
| Same policy revision, selected item with a different target rule | Recompute local effective policy and request membership before measurement. New questions are answered and forbidden controls disappear. |
| Same effective policy/minima, offscreen selected tab | Reveal/scroll updates the actual bar/receivers; untouched leaves remain shared. Initial slice deliberately falls back if receiver geometry changes. |
| Select one leaf while another leaf's measured minimum or intrinsic changes | Complete Q discovers the unrelated change and rebuilds its affected geometry; local edit information does not hide new measurement dependencies. |
| Reordered items or moved tab in a candidate drop | Future layout reads its retained source-origin metric slot; it never indexes the new item order using an old array ordinal. |
| Active menu row clipped outside pointer viewport | Keyboard/accessibility semantic reachability remains distinct from pointer rows. Do not derive semantic recipients from pointer visibility. |
| Changed scroll near splitter/neighbor touch chrome | Compare touch fragments with conservative output, including first subsequent touch. No stale global touch index after a changed input. |
| Press selected tab then release from the same journal | Admitted owner-held capture continues correctly; the old emission cannot start a new unrelated press after revocation. |
| Candidate encoded but not observed, dropped frame, or final-hook paint mutation | All reject interactive admission. A previously retained output keeps only the authority the owner explicitly retained. |
| Old receipt after selection, structural edit, restore, host rebind, coordinate transform or retirement | Emission/root/host/native admission rejects stale input. A return to equal values does not revive its grant. |
| Failed first-live native create, source edit during staging, source recovery | No staged facts authorize input or transfer source ownership. Existing lifecycle conformance scenarios retain their outcomes. |
| Candidate selection then rollback; A-B-A and MRU changes; repeat pass failure | Original state remains on rollback. Initial/final publication semantics match actual MRU/grant/fallback state; no intermediate OR flag reports a false no-op/change. |
| Combined contained occlusion, invalid viewport, missing Visible tab, lost drop coverage | Retain the current geometry rejection cases and reference checker. Private typed rows alone cannot establish these invariants. |
| Foreign wrapper, passive or absent receiver role | Direct links return the same absence/error semantics without a fallback semantic-name search. |

Retain two test layers: small owner/Adapter scenarios through `begin/present/finish`, and a test-only independent conservative full constructor used as an oracle for changed facts and normalized complete output. Do not keep two production representations in sync to preserve the oracle. Existing public historical-proof tests that only encode the removed value/ABA contract should be rewritten around current grant rejection rather than forcing the obsolete representation back into production.

Run the established nextest and native/host conformance suites serially during implementation. For performance, retain complete callbacks, all pane content, full program recording, output settlement and complete product tessellation. Count avoided visits/allocations separately from timing and report retained current/fallback/in-flight memory. No speedup or millisecond floor is established by this design.

## Depth, Leverage and Locality

The Interface owns the hard ordering: admit edits, ask full metrics, update only actual dependencies, emit every operation, validate final completion, and publish atomically. Callers do not manage tickets or remember which independent derived view to refresh. That creates Depth and gives both real Adapters Leverage.

Selection dependency knowledge lives with the editing owner and the leaf constructor. Output/native admission knowledge stays with the existing lifecycle owner. Their Seam is internal, concrete and in-process. This gives Locality: changing a selection's consequences is one owner/leaf change rather than a new fingerprint rule, manifest mutation, compiler reconciliation, installation validator and Adapter join.

The deletion test is concrete: removing this Module would force both egui and headless callers to orchestrate measurement completeness, local invalidation, geometry/semantic linking, current grants, paint settlement and rollback. The current detached ticket protocol mostly redistributes those chores; this replacement absorbs them.
