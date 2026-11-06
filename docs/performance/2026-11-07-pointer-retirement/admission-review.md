1. **Confirmed baseline defect: journal retention incorrectly depends on current admission.** In `crates/dockspace/src/runtime/native/pointer.rs:1110`, `validate_pointer_binding` requires the exact provider plus `binding_is_live`. That rejects an exact producer-owned incarnation whose `admission` has become `None`, although its callbacks and route sidecars have not quiesced. The same helper is reached by desktop hover, event delivery, capture, and previously captured local-coordinate samples. The retained callback in the causal RED contains delivery `first` and hover/capture `second`; `second` is explicitly noncurrent but recognized. Root's execution receipt reports successful compilation followed by the expected `Native(InvalidPointerFacts)` at `coordinator/tests/retirement.rs:822`. This is a confirmed retention/admission mismatch, not evidence that the retired child has current action authority.

2. **No source-level blocker found in the bounded fix candidate reviewed below.** Its journal helper accepts only `recognizes_binding`, while `compile_pointer_roster` separately retains the old `contains_binding` requirement. Fresh coordinates, current/focus queries, reducer routes, presentation checks, work-area validation, provider checks, and ordering remain unchanged. This is the recommended minimal fix. A helper-only substitution without the roster guard would widen the documented recovery-roster contract and should not be accepted as the same bounded change.

3. **Qualification remains with Root.** I examined source and Root's existing RED receipt; I did not run Cargo, compilation, tests, benchmarks, or timing. The reviewed candidate's added assertions cover the reproduced callback, workspace-version stability, absence of an active gesture, the roster guard, and rejection after forgetting or from another provider. They do not dynamically establish retired delivery, retained local samples, same-surface successor/ABA behavior, or active release cancellation. Those are the remaining focused verification cases, not reasons to introduce another validator or state model.

**Scope and evidence identity**

Repository: `/Users/frankorz/codes/rust/egui_docking`. This is a bounded native pointer lifetime/admission review. Only this report was written. No repository, candidate, fork, or cache changes were made, and no agents were spawned.

Production references below use the base line numbers at immutable commit `3653b98e0ee6aa71e8e12ad61dd66fd6ad9d48f8`. Every production file in the source table was byte-compared with that commit and matched. The retirement test is different: its reviewed base is the immutable original plus the separately applied causal RED, not an immutable test change already contained in that commit.

| Evidence | SHA-256 / identity |
| --- | --- |
| Original retirement test at the immutable commit | `614b1875cbae55a96a3e6074639cfa98ba51e8e28d53338248aadd6887b6b493` |
| `/tmp/physical-pointer-retirement/causal-red.patch` | `14803257fbb036253abdba2cf2d7e13f64f20dbdbb7137cfaee2846d965dbcd3` |
| Applied RED / fix-candidate base retirement test | `4bb735753022016ee52ea505dd8e988846e574be1748db3b38823d5ba43dd17b` |
| `executed/causal-red.log` | `5f1987fae1971edd3c2f9c5781ba81421c4e0877141178a54ae85f03a8590744` |
| `fix-candidate/capture.json` | `b020c1f2ffc07bc3e01ebc1d5cea71df7b61476d99dd1ab33374be97eebb3645` |
| `fix-candidate/fix.patch` draft snapshot | `c4dee0e2a4a18671b63fb6dbe5d928b23647ebc5bc40a9368b5a2674fa260165` |
| Candidate `runtime/native/pointer.rs` | `852e72b524d6fdbf532b8e05be052b2c3b0ad56cfcbff91d768d28ac6eb946d5` |
| Candidate `runtime/native/session.rs` | `5182741a8c7dc9eb005ba44e555cf0b929252f1b8161150f341f1388e926845f` |
| Candidate `coordinator/tests/retirement.rs` | `4eed864bf316a80a4e3cc3b95b6c47cbf3c59fc4a02a72885ebaef0d46cfc0a0` |

The candidate was inspected as a draft; no READY or execution result is implied. The final re-read of the session tree included three comment-paragraph separators added after the patch snapshot above was generated (earlier session tree hash `92392c31255cf604f13b46cf6086d670acfec829be79f195c0d0af3cf87b9c33`). I inspected that comment-only refresh; the table records the newer tree bytes. Final packaging should regenerate the patch and bind its final digest. The production delta is confined to the journal predicate, the roster guard, and documentation clarifying observed incarnation versus current authority. It contains no reducer or adapter-runtime changes.

**Why the history is legitimate**

`RuntimeNativeState::recognizes_binding` (`runtime/native.rs:1152`) requires the current platform provider and an exact `ViewportBinding` in the retained producer. `NativeProducerState::binding` (`:994`) uses binary search on the full binding. `prepare` (`:1041`) keeps preceding entries and assigns `admission = None` when the current registry no longer has that exact incarnation. A surface successor is a separate entry; the old binding is not rewritten to the successor.

`binding_is_live` (`:1156`) only requires `admission.is_some()`. It is not the full fresh-action predicate: Pending or Retiring entries with an admission record may still satisfy it. `contains_binding` adds the exact provider. The proposed roster guard preserves this existing identity condition exactly; it does not replace it with the stronger `Admitted` action condition.

The adapter can already own callbacks before admission ends. `NativeWindowEventRecord::from_parts` (`egui_dockspace_native/src/event.rs:449`) freezes delivery, hover, and capture routes using the callback-time map. `viewport_map.rs:623` suppresses future pointer routes and clears cached local coordinates; it cannot rewrite the owned earlier record. In the RED, raw callback ordinal 13 retains all three exact routes across `accept_native_effects`, while `is_current_native_binding(second)` becomes false and recognition stays true (`retirement.rs:775–818`). Rejecting that event at recorder time prevents draining the real retained prefix.

No identity is manufactured by recognizing this history. `NativeSurfaceBinding` has private provider/binding fields (`runtime/native.rs:97`), and full-incarnation lookup is retained. A producer cannot submit another session's binding merely because its visible surface or window token matches.

**Fact ingestion versus fresh permission**

| Input role | Retention condition for the proposed journal fix | Independent action condition that remains |
| --- | --- | --- |
| Desktop dock hover | Exact provider-owned, recognized incarnation | `DesktopRouteFact::validate_against_registry`: current exact registry binding and `is_routeable()` |
| Desktop native delivery | Same exact recognition | Delivery-derived route plus press/current-owner checks; hover is not substituted for delivery |
| Native capture | Same exact recognition | Exact required gesture owner and current native binding; loss can cancel |
| Retained local sample | Exact recognized binding **and** exact joined backend lease | Current presentation endpoint, current routeable binding, coordinate generation, and original logical conversion |
| Recovery roster capture | Existing exact `contains_binding` check remains | Recovery boundary/provider/watermark checks; a roster supplies no press origin |
| Work-area route | Existing current provider/token/generation check remains | Existing reducer work-area authority; only explicit OutsideAll may carry the route |

The journal path is `compile_pointer_journal` (`pointer.rs:867`), which resolves location, independent delivery, and independent capture before building the edge. Its location subpaths reach the helper at `:1014` and `:1040`; owners reach it at `:1082` and `:1099`. The fix changes the acceptance of the opaque window lifetime, not the edge's location, role, metadata, or sequence.

Explicit `Foreign`, `None`, and `Unknown` facts retain their meanings. “Foreign binding rejected” here means a `NativeSurfaceBinding` issued by another provider/session, not an explicit `NativePointerHover::Foreign` or `NativePointerOwner::Foreign` classification. A retired Dock fact must not be rewritten to Foreign, Unknown, OutsideAll, or a successor's Dock binding to make admission succeed.

**Current action checks in the reducer**

For desktop input, `validated_desktop_delivery_route` (`engine/pointer_proof.rs:330`) combines the independent physical position with the edge's exact native delivery binding. It deliberately ignores hover classification. `DesktopRouteFact::validate_against_registry` (`pointer_journal/desktop_route.rs:299`) checks authority domain, registry presence, full binding equality, and routeability before deriving a dock route. `ViewportRecord::is_routeable` (`viewport_registry.rs:159`) requires Admitted, ready, and visible. Missing, retiring, or same-surface successor records produce typed unavailability; they do not retarget the old child.

A new `GestureOwner` for desktop input requires the validated dock route (`pointer_proof.rs:502`). A native-local owner requires `validated_native_local_route`. Without an existing owner or one of those valid routes, ordinary press/move/release edges reduce to no UI interaction (`pointer_transaction.rs:130`). `reduce_journal_primary_press` additionally requires a valid logical point, exact delivery, a current semantic presentation/receiver, and compatible capture (`:483`, `:517`, `:531`, `:537`). Thus a stale native delivery cannot create a Pane interaction or a fresh gesture merely by entering the journal. A callback with a separate valid live delivery may still act on that live endpoint; history acceptance does not globally disable independent valid authority.

For an existing gesture, `native_capture_binding_is_current` (`pointer_session.rs:6`) requires a surviving workspace surface, exact registry binding, Admitted, and observed. The move and release reducers call delivery and capture gates before action work (`pointer_transaction.rs:1036`, `:1310`). An old/mismatched delivery loses delivery authority; an old native capture loses capture authority. `reduce_journal_capture_change` (`pointer_session.rs:232`) can cancel the exact active owner when a known capture is no longer current. Unknown capture does not revive a lost native owner. Release's post-edge `None` handling retains its exact pre-edge/current-source conditions; the fix does not bypass them.

Native-local continuation has an earlier safeguard: a matching active local owner with no current validated local route is cancelled with `CoordinateAuthorityUnavailable` (`pointer_transaction.rs:196`). Timed work and the local-to-desktop handoff also require the exact current coordinate source and real primary-press continuity (`pointer_timed.rs:21`, `pointer_handoff.rs:82`). A roster's held buttons have no delivery origin (`pointer_journal/ledger.rs:129`), so a recovery capture cannot manufacture that handoff proof (`:453`).

The pointer helper also serves scroll journal ingestion. Initial spatial scroll candidates derive local/desktop positions from the same current route validators (`pointer_proof.rs:115`); an unavailable route supplies no fresh dock target/point. A new owned receiver requires a known current presentation and an endpoint joined to the exact delivery owner (`scroll_interaction.rs:883`, `:1001`). Locked challenges require an existing owned scroll session (`:292`), and its continuation checks exact endpoint lifecycle before applying a delta (`:660`, `:1344`, `:1436`). A detached old binding cannot pass the registry equality check; a same-surface successor cannot inherit it. A suppressed scroll session may retain bookkeeping, without becoming an owned receiver. Contradictory explicit scroll endpoints retain their existing typed errors; the fix does not promise that arbitrary contradictory payloads succeed. No reducer change or second coordinate executor is required for this lifetime fix.

**Local-coordinate capability boundary**

Fresh `native_local_coordinates` (`runtime/native/session.rs:143`) still requires a managed joined backend equal to the engine's active ingress, `contains_binding`, and `native_local_coordinate_snapshot`. The latter (`pointer_proof.rs:538`) requires the active presentation host/endpoint to name that exact binding, a matching routeable registry record, and positive content size. Admission loss therefore cannot mint a new current coordinate capability.

A previously issued `NativeLocalCoordinates` is copyable. Its `capture` method (`local_pointer.rs:156`) validates exact binding, callback/event size and native scale, the complete frozen geometry, and finite conversion by the original presentation scale. It does not consult current engine state or cryptographically date the OS callback. Retaining such a capability/sample after retirement is therefore possible under the existing trusted backend contract; this is why the current reducer check is essential.

`native_local_pointer_route` (`pointer.rs:1006`) preserves the sample's client point, original logical point, and coordinate generation. It first rejects a different joined backend lease. After recognition-based retention, `validated_native_local_route` (`pointer_proof.rs:44`) still reacquires current authority, requires the exact coordinate generation, and checks the logical conversion. The sample is not reinterpreted using a successor's scale or promoted to desktop/work-area authority. Keep capability issuance strict and permit faithful transport of an old exact sample; adding a timestamp proof, trust flag, cache, or second coordinate executor would not improve this boundary.

**Roster, quiescence, and order must remain separate**

`compile_pointer_roster` (`pointer.rs:755`) shares `capture_owner` with journal compilation. The public roster contract (`session.rs:265–275`, `pointer.rs:179–181`) requires current native capture. The candidate's explicit `contains_binding` guard restores exactly the pre-change acceptance set at that call site before it reaches the widened helper. Initial enrollment still rejects all Native capture (`pointer.rs:1145`); provider enrollment cannot refer to a native capability it has not yet minted. `report_native_pointer_roster` still rejects superseded providers and pending native input, and validates the recovery boundary/current pointer watermark before appending (`session.rs:289–303`).

The candidate's roster-negative assertion calls the public API after the historical callback commits. That fixture need not have an open recovery boundary: current implementation compiles the roster before checking that boundary, so the exact `InvalidFacts` assertion exercises the new guard rather than passing through a generic resync failure. It does not prove the positive recovery-roster lifecycle; retain the existing overload/resync tests for that claim.

`record_binding_quiescence` (`runtime/native.rs:1243`) checks exact provider, rejects a still-live binding, records quiescence successfully, then removes the exact producer entry. Recognition ends on that successful append, even before a subsequent engine commit. A failed append keeps recognition available for retry. The new journal helper must reject the forgotten binding afterward, including if a same-surface successor exists. No lifetime may be forgotten merely because the current layout vacated the surface.

Backend and provider order are unchanged. The raw callback ordinal 13 in the RED is a mailbox identity; it is not the same counter as the provider pointer edge sequence or backend ingress ordinal. `compile_pointer_journal` assigns the next provider edge sequence. `record_pointer_segment` (`backend_ingress.rs:1383`) requires exact previous continuity and updates the pointer watermark only after successful append. Ledger preparation rejects replay/ahead journals (`ledger.rs:858`). Sequence preparation finishes before append, and savepoint rollback preserves all-or-none admission (`pointer.rs:781`, `:820`; `session.rs:565`).

The adapter claims the immutable cursor prefix, records the sequence, and only then transfers/acknowledges its translated prefix (`coordinator.rs:1685–1728`). Destroyed records append exact input retirement before translator retirement/normal lifecycle acknowledgement (`:1734`). `observe_native_input_binding_retired` (`engine/native_input.rs:70`) checks the active joined providers and a retained exact lifetime, then retires only the physical streams dependent on that binding; the ledger preserves unrelated streams and button knowledge (`ledger.rs:593`). Accepted pending release obligations remain independently owned. The repair must neither drop/backdate the retained callback nor move terminal or replay barriers around it.

**Focused verification still needed; no new framework**

- Root should obtain GREEN for the causal retirement test with the fixed production bytes. Preserve the frozen-route/ordinal assertions, admission-versus-recognition assertions, unchanged workspace version, no active gesture, exact input retirement, suppression of later snapshots, translator cleanup, and eventual quiescence. The RED receipt proves the pre-fix failure only.
- Cover the three journal roles independently with an admission-None recognized child. In particular, submit old **delivery** rather than only the RED's live delivery plus old hover/capture; assert faithful journal retention, no new receiver/gesture/workspace mutation, and cancellation rather than execution for a matching active move/release whose source has lost authority. A small table of role cases in an existing fixture is sufficient.
- Retain one real local sample before loss of admission, then submit it afterward. Assert no fresh `native_local_coordinates`, no new gesture/handoff, and no conversion through newer geometry. Repeat with a same-surface successor/changed coordinate generation to cover ABA and stale-coordinate rejection at the reducer.
- Preserve provider/backend mismatch, forgotten-after-quiescence, stale work-area generation/token, work-area attached to Dock/Foreign/Unknown, explicit Foreign/None/Unknown distinctions, and initial Native-roster rejection. Negative batches/sequences must leave the recorder, pointer watermark, and retained prefix unchanged. Existing foreign-handoff atomicity coverage (`pointer.rs:1423`) and cleanup/fresh-coordinate coverage (`tests/managed_lifecycle/report_backpressure.rs:93`) remain relevant.

The candidate's forgotten/provider-negative loop currently encounters an invalid hover before its capture field. It proves rejection of those inputs and unchanged record count; it does not independently exercise each owner branch. This is a coverage limit, not a source defect in the shared exact-recognition predicate.

**Source hash basis**

Paths below are relative to the repository root. All listed production bytes matched immutable `3653b98e0ee6aa71e8e12ad61dd66fd6ad9d48f8` when checked. The pointer/session candidate bytes are separately identified above.

| Source | SHA-256 |
| --- | --- |
| `crates/dockspace/src/runtime/native.rs` | `0186d446fd01e2ad5bad2841746774d6a2a4c769c72b4b2fa23d04fb04d3c4f6` |
| `crates/dockspace/src/runtime/native/pointer.rs` | `4f830aa26aea09c5d0cf63c1a81309e7f05a7bbe9ce4d0a22b3a9b6697ebc03e` |
| `crates/dockspace/src/runtime/native/session.rs` | `3d1ee57951473d89afa63a7c7b2b62d0f9ded01ba3c192b8c0b544b9caf795e5` |
| `crates/dockspace/src/runtime/native/local_pointer.rs` | `0bcf9500dc3e261219ad3882c6ac231f79a957919aaf6061760ed2d980880ba8` |
| `crates/dockspace/src/runtime/native/receiver.rs` | `0d1bd7a3b98a1d3e1bfdbb0aac92dd7e20ad9ff547e442bd767d6e64725d31fa` |
| `crates/dockspace/src/pointer_journal/desktop_route.rs` | `562d601ab68ba5c0d80fc242ee00d4915263d50e3bba4a555b363ba8465fdbc2` |
| `crates/dockspace/src/pointer_journal/ledger.rs` | `6cd901a80158ae50f9c0253ae74b20b722219f247cd45d0923338e27e92ee8d4` |
| `crates/dockspace/src/engine/pointer_proof.rs` | `00a1e91098a8742e25f1a9a7b9d799a13be1540a9df6749226f92108a44b150f` |
| `crates/dockspace/src/engine/pointer_session.rs` | `d3ec65c2cd54bfa909b2dda73462eb3524ff3df8cdd166390ce34f970048e978` |
| `crates/dockspace/src/engine/pointer_transaction.rs` | `d789f45614c0b49653b58d2f3c3959a328797aadb7af57a3cfcbabdb1c2c5021` |
| `crates/dockspace/src/engine/pointer_handoff.rs` | `b9ba6113b7ea41b52621ed95491901ca37a09f5ac0aa6584f585f846a1ac0aa7` |
| `crates/dockspace/src/engine/pointer_timed.rs` | `02de961a7cd8850bd1e8aab31ea89012ae203cafe91c96d1d45f90c753d9449c` |
| `crates/dockspace/src/engine/native_input.rs` | `b2b86417a5490b15ab57183fa3d262b6ca3194fee0c513638057e96c4f3551a4` |
| `crates/dockspace/src/engine/scroll_interaction.rs` | `8acf19037995c75973e4d3edb71ed8d966f3507404175f087fa0ceb84b59bdae` |
| `crates/dockspace/src/backend_ingress.rs` | `451dc342c405a8157f22cc493d6ab721ea62c66e13d1ebf64c0b78217fb93ae5` |
| `crates/dockspace/src/frame.rs` | `e0ff01589c4c40d045b946561bc3ca818e5bfe732f01ad6fdd9efc74c16bc3c2` |
| `crates/dockspace/src/viewport_registry.rs` | `0df93a6fb3bd633edb9459a56be72086f0b0e1283b81d407d376150211c00673` |
| `crates/egui_dockspace_native/src/event.rs` | `a57b1aa1cdd5eedffb9c58ec127583889e20c785bfd5afe1fbd5ff25d2dd867c` |
| `crates/egui_dockspace_native/src/viewport_map.rs` | `f0e61a34bfbad0bb4d77732b1b966899b967e809531c3e3ece50d8e1157cc97b` |
| `crates/egui_dockspace_native/src/coordinator.rs` | `e592e5d9e5585710570ab5cf1354cdc682473a71545005e7252566ad320e25e1` |
| `crates/dockspace/src/runtime/native/tests/managed_lifecycle/report_backpressure.rs` | `3954f67be2d634e870933c51dc6e280dcbf1870e545d21c96f4af1b14ce2521a` |

The source-supported conclusion is that exact recognized lifetime is sufficient to retain these journal facts, while existing current geometry, delivery, capture, and presentation gates must remain the sole route to actions. Root's RED demonstrates the admission mismatch in the retained-callback fixture. Reproduction of the original physical run and GREEN qualification of the final candidate remain separate execution claims owned by Root.
