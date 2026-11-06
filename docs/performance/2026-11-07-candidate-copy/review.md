# Independent review: candidate Node/MRU payload sharing

Date: 2026-11-07. Verdict: **no blocking source defect found in the exact copy-after-prepared-report candidate reviewed below**. It is suitable for root's application and compilation gate; this is not a qualification or performance result. The reviewer did not author either the copy slice or the prepared-report slice.

The repository and supplied evidence were only read. No repository edit, patch application, compiler, Cargo, test, benchmark, timing or Git mutation was performed. The only written artifact is this review. Patch replay described below ran entirely in memory and did not execute Rust.

## Findings

No correctness blocker was found in mutable access, replacement/retirement, generation handling, issuer separation or candidate isolation. The proposed concrete stores are a bounded representation change and do not introduce a generic storage framework or another executor.

One nonblocking test precision improvement is recommended: in `after-prepared-report/crates/dockspace/src/graph/copy_tests.rs:257`, `a_late_batch_failure_discards_detached_payloads_and_preserves_original_refs` checks only `is_err()`. Assert the exact error value `TransactionError::Command { index: 1, source: CommandError::ItemAlreadyOpen { item: ItemId::new(2) } }` instead. This identifies the intended second-command failure after the first general-path move, rather than also accepting an unintended first-command rejection. Keep the existing complete-value, pointer, original-reference and successful-retry assertions. This is a small strengthening of the new test, not a found runtime defect or a request for another test framework. The independent `copy-tests-precise-late-failure.patch` supplies this assertion after either main copy variant; it does not alter either main patch or its frozen output copies.

## Exact review subject and provenance

The review subject is `candidate-copy-after-prepared-report.patch`, not the alternative patch for bare A1 inputs. Its input includes the separately reviewed prepared-report proposal. The live workspace had not applied prepared-report or copy when checked, so differing live hashes for operation, transaction, resolver and their migrated tests are expected. They were not treated as candidate provenance failures.

| Artifact | Independently calculated SHA-256 |
| --- | --- |
| `README.md` | `447196281d343570bfd25c3e5d0b6a4ecc0251a00215a3a58b84084050ae8a12` |
| `candidate-copy-after-prepared-report.patch` | `9a4e41c222741a944e23ddaf5a45e21a27bd2cec9d76646abe9937116342e2dd` |
| prerequisite `candidate-report-slice/candidate-report.patch` | `1b0a17402068d3b2d782bbe5e39538cb3366e30f6b0a45e95236293e815425b9` |
| `after-prepared-report-base-sha256.json` | `5af25ef242c2d21990f2b1c5ffafe06ad8356272571411a6a29c435ed60d7895` |
| `after-prepared-report-sha256.json` | `bcfb3bdf6e186904321dbe98d111f02893f2996d2a5869c5c1d2fa75b0a7203b` |
| `contract-sha256.json` | `72784c3b1083f322b633ff5a02128025b40be64de36be9f5365dad5070fa6dd5` |
| seven existing input files, aggregate | `35773b3c9985065df539346099b00dc67012423efa53eb853bdf30e173e12ca4` |
| nine candidate files, aggregate | `4dd4ce0ce630d901aeedf0b6fc9697072eecdb0dd3a9f8056f152557fb882568` |

Aggregates concatenate sorted repository-relative path bytes, NUL, then complete file bytes. The base aggregate includes seven existing files; the two required absent paths are explicitly recorded as null in the input manifest. Every input was located by its exact hash in `candidate-copy/base/` or `candidate-report-slice/proposed/`. Every unified hunk was replayed with exact line/content checks; all nine resulting files equal the complete copies in `after-prepared-report/` and their recorded hashes. The patch inventory is exactly **9 files, +575/−39**, including two new files and seven new registered unit tests.

The five unchanged live dependencies in `contract-sha256.json` match their recorded hashes: graph authority, workspace/reference capture, validation, persistence and the frozen layout reference. In particular, `layout/reference.rs` remains `93afdf6e8ad79d81f2efdafc4814a4bb6076fd9f535129f82ce33f9ee12ddf4e`. Neither layout/leaf materialization nor docking-cost timers/export/workloads are edited by this patch.

## All mutable payload access is closed by the stores

`graph/shared_storage.rs` keeps both underlying containers private. Read methods return ordinary shared Node/Vec references. The only methods that return mutable payload references are NodeStore::get_mut and TabMruStore::get_mut, and both call `Arc::make_mut` before granting that reference. The shared data contains flat IDs and owned Vec values, without interior mutability. Safe Rust's reference lifetimes prevent a store clone or competing mutation while such a mutable reference remains borrowed.

I checked the representation's callers across the crate, including multiline accesses and raw diagnostic fixtures. Existing partial writers in select, reorder, split resize, item removal/insertion, node merging, split-child changes and wrapper replacement all continue through the stores' get_mut methods. Raw Builder fixtures in layout, canonicalization and engine tests also use that same mutable entry. The sole raw Node IndexMut fixture is explicitly migrated; the canonicalizer's raw-container for-loop is changed to `.iter()`. Other `.nodes` iter_mut or indexing occurrences refer to separate fingerprint/topology/snapshot vectors, not the normative Node store.

No mutable Arc, DerefMut, mutable iterator, mutable retain predicate, raw SlotMap or raw MRU BTreeMap is exposed. The remaining container API used by current readers is present. Public Workspace node/nodes/tab_mru value access is unchanged. Persistence and the frozen solver continue reading the same logical values. The extra TabMru newtype permits observing its actual Clone event and adds no second canonical history or allocation layer beyond the one Arc payload allocation.

## Replacement and retirement do not detach discarded payloads

NodeStore::replace swaps the Arc slot directly. The canonicalizer already owns the complete new split vectors at that point, so its whole-record replacement does not first clone the shared split it discards. This preserves the previous normalization math, error construction and rewrite order. No unchanged split is sent through replacement by rewrite_split's existing early return.

NodeStore::remove returns the removed handle. The iterative subtree remover inspects it by reference and pushes copied child IDs in the same reverse order as before; it does not materialize a full removed Node. NodeStore::retain receives a mutable handle from SlotMap internally but exposes only `&Node` to its predicate. TabMruStore retain similarly exposes only `&Vec<ItemId>`. Canonical sweeping therefore retains or drops handles without copying either surviving or removed Vec payloads. MRU removal returns presence because current production consumers discard its old value; the one raw fixture that needs an owned history explicitly obtains it.

Builder replace_node deliberately differs: its public contract returns the previous owned Node. `Arc::unwrap_or_clone` preserves that result by moving an unshared old value or cloning the shared one. That clone is necessary to retain another owner's old payload and is included in the Node Clone counter. The new record and its selected-first initial MRU are still installed through the same Builder contract. A stale/missing key still returns MissingNode.

Children remain NodeIds, not owned child Arcs. These stores introduce no recursive clone/drop chain, so they do not undermine the existing iterative deep-tree design.

## Generation, authority, placement and rollback

The candidate independently clones stock `SlotMap<NodeId, Arc<Node>>`; it does not share the allocator. I inspected the pinned slotmap 1.1.1 implementation: occupied slots clone their value handles, vacant slots clone their free-link state, and the slot version/free head/element count are preserved. Insert/remove/get/get_mut/replace retain the complete-key generation checks of SlotMap. The MRU BTreeMap is also independently cloned and remains keyed by the complete NodeId. A branch can retire/reuse a slot without modifying the original allocator or its live old generation.

Storage sharing does not issue command authority. Graph authority and all concrete completion calls are unchanged. Immutable Workspace clones still inherit existing root references; a real edit replaces issuance through the existing owner, and an independent Builder completion issues fresh refs. Payload address equality is never substituted for RootReference equality. Strict ABA, source-first checking, separate same-root target validation, unrelated-root preservation and selected-item policy derivation therefore retain their A1 owners.

Normative placement remains a separately owned, cloned directory, updated by the same concrete operation/canonicalization helpers. Replacing a stored Node handle does not change its NodeId or skip any placement update. The patch does not introduce a new branch equality or a new way to accept a stale source/index.

On a failed batch, allocator/map/directory mutations belong to its candidate and every shared Node/MRU write has detached first. Dropping the candidate cannot mutate the original payloads or original issuer map. Publication still assigns the owned completed Workspace at the existing point; it does not postpone releasing the replaced Workspace beyond public apply. Prepared candidate/report immutability from the prerequisite is preserved: the copy patch changes only removed-subtree consumption in operation, not construction, report classification, staging or publication.

Existing geometry/output facts remain owned immutable data. Workspace snapshots can additionally share untouched payloads safely; later writers must detach them. Old outputs do not borrow a Node through an unchecked raw pointer, and no output authority predicate was changed. This storage slice neither alters nor supersedes the separately reviewed resize-projection initialization-order fix.

WorkspaceData equality still compares exact live NodeIds and Node values. Its MRU comparison now traverses Arc<TabMru> values, whose exact ID-vector Eq may safely shortcut identical handles. Node comparisons still use `&Node` value equality rather than Arc identity, so raw invalid float comparison behavior is not silently converted into pointer equality. Clone preserves weights and selection bits; it performs no numerical recomputation.

## The work ledger distinguishes three different quantities

The retained legacy workspace_deep_clones fields count explicit fork calls and source logical inventory. They do not become zero and do not claim that all N item/child/MRU entries were physically copied. Direct caller-owned Workspace clones still fall outside those explicit call-site categories as before.

The new node/mru store counters run at actual concrete store Clone calls and count the live handles in their cloned source containers. SlotMap's inspected Clone implementation copies each occupied Arc once. The live handle count deliberately excludes vacant initialized slots, capacities and container allocation bytes. A candidate fork remains proportional to allocator extent plus live handles and the other cloned maps/directory; it is not O(1).

Node::clone and TabMru::clone record actual payload Clone events and copied Vec lengths. Unique make_mut incurs no such event. A Node value explicitly cloned by a fixture or an owned Builder return is also counted; arbitrary domain Vec snapshots, fingerprints, newly built insertion/rewrite vectors, authority/placement copies and temporary child-ID stacks are outside these counters. These are correctly documented limits, not absent work.

All hooks use the existing test-only update path and existing `DOCKSPACE_BENCH_NO_WORK_COUNTERS` compile-environment bypass. No formal timer or CSV/workload/schema is changed. The seven test assertions are source predictions until executed. In particular, the two-node/two-MRU frontier asserted for the populated-source center case is not generalized to singleton retirement, no-ops, edge repair or all general batches. Equal selection/resize writers can request mutable access and cause a real local copy even when durable values remain equal.

The proposal also adds one Arc allocation/header per Node and per MRU record, atomic handle clone/drop work, and pointer indirection during traversal. SlotMap buffers, MRU map nodes and all remaining metadata still copy. Complete operation timing and allocation evidence are required to determine the tradeoff; neither reduced Vec copying nor this review establishes a numeric speedup or total memory improvement.

## Qualification boundary

The seven tests are registered beneath graph's cfg(test) module and exercise real normal-owner/Builder paths: fork sharing, populated-source center mutation and unique follow-up, raw write isolation, immutable sweep/removal, owned replacement return, late rollback and complete-generation branch reuse. They supplement rather than replace the existing owner/staged/placement/strict-ABA cases and full local/general semantic oracle. No prior success or rejection assertion is removed by this copy patch.

Root should first apply the prerequisite and the exact matching copy variant and typecheck all feature/test callers. Then run the seven cases, existing local Item and selection matrices, owner/staged/placement/ABA tests, both float-frontier witnesses, deep-tree/frozen-layout oracles, and normal workspace/serde/host/native/prepared/old-output suites. The existing complete workload must still supply fresh layout inputs, complete exports, original destruction and full-state matching in the same timer scopes. Broad existing checks are preferable to a new copy-specific harness.

This review establishes source closure and exact artifact composition only. The candidate remains unapplied and uncompiled in the reviewed evidence; only root's later checks and frozen comparison can qualify it. The separate cold-effects candidate intersects operation/canonicalization and must be explicitly rebased if selected later.

## Applied-candidate augmentation, 2026-11-07

This section updates the earlier temporary-candidate status for the actual applied copy slice. It does not repeat the main representation review or qualify performance. **No blocking augmentation defect was found.** The reviewer only read current source and root's existing artifacts; no repository edit, Cargo/test execution or timing was performed.

The main after-report patch remains `9a4e41c222741a944e23ddaf5a45e21a27bd2cec9d76646abe9937116342e2dd`. Eight of its nine complete output files equal the live source byte for byte. The ninth, graph/copy_tests.rs, equals exactly the saved precise late-failure supplement. The separate docking_cost.rs addition equals its saved work-CSV candidate on top of export-reader. There are no unexplained changes inside these reviewed ten paths.

| Path under `crates/dockspace/src/` | Actual current SHA-256 | Relation to reviewed source |
| --- | --- | --- |
| `graph.rs` | `4d99bd0afcf7bd37582e098cb10330921d8db631914ebdcdac2477809e9eef80` | Exact main output |
| `canonical.rs` | `b3d8964ad12ba385c222d1b27019a0fb70ba48801d57d1efbc4532a897dc5d81` | Exact main output |
| `operation.rs` | `e093aecc1356ef1e37cfdc77747546d403935e16f9e753f318aedaeecdc26f02` | Exact main output |
| `drop_resolver.rs` | `dd5635758ccec5e86f8086b2e5cec97c024857b942a2e71fd80a3076b1cc19f3` | Exact main output |
| `transaction.rs` | `0970cc5343f6eec5bc300603e78303782b3e817edb86064b3694fac4544106a2` | Exact main output |
| `operation/tests/local_selection.rs` | `4cf3d43305ff3b9caf87bebb47d9efeb1b77292152c7b1da37e94ff5a05edc9d` | Exact main output |
| `operation/tests/owner_reference.rs` | `8a30c1a5b53ae5571cae8e0354f4759bb50c082acf5bfb27915c71f79f967edc` | Exact main output |
| `graph/shared_storage.rs` | `47a848dd8b1be7f48a03af00c74cc9ae28478a7dff6e79b0d3933c389e5e9b09` | Exact main output |
| `graph/copy_tests.rs` | `ca978a02a67acb2e93aef8b7080f0de5ea1c59b6f1564b47a1dc793f591a7832` | Main plus exact index-1 failure assertion |
| `docking_cost.rs` | `28ecb77733c55d60c85704cd89e85e24ab41fb6d851cdfc00e8cc6fb217ddaac` | Export-reader plus ordinary work-CSV columns |

The precise assertion supplement SHA-256 is `3115fb4a71a5d9475b1d68aa93378c1310a368775744e20311453e1791b339b5`. Current source asserts the complete `TransactionError::Command { index: 1, source: CommandError::ItemAlreadyOpen { item: ItemId::new(2) } }` value, then retains every original equality/pointer/reference/validity assertion and successful retry. Root's targeted.log explicitly reports this exact test passing at 45/124; core-production.log reports it passing at 1290/2058. Both completed source manifests include its current hash above. The seven new copy tests all appear as actual passes in both logs, rather than only being compiled or listed.

The work-CSV supplement SHA-256 is `ce38928cd72080e77526e63955ce5670e31063375e2348b2f16f8973002a1801`. Its exact base `5874ffaf0a9f80e75d07b22ca28a031789bcbe865dda6613d6b93a6a159b6b83` equals export-reader's docking_cost.rs output. Every byte before fn docking_work is unchanged, including the complete formal docking_cost test, CSV schema, environment requirement and the measure/fixture/export implementations.

Only the existing ignored ordinary-counter docking_work body appends fields. Its original ten columns and expressions remain in order; ten named workspace_storage fields are added, making **20 total columns**. Each field maps once to its actual NodeStore/MRU/Node Clone counter; there are no new counters, fallback zeros or inferred physical-copy formulas. The normal-counter compile guard remains required. This is an attribution schema addition, not a formal timing-schema change.

The existing fixture still precedes reset. There is exactly one original reset, the same complete measure call, and the original snapshot immediately after measure. Validation, fixture destruction, expected-state checks and CSV formatting remain after snapshot. Thus the appended fields cover the same complete request/mutation/layout/export sample as the legacy counters, not mutation alone, and exclude fixture setup and post-snapshot release/validation. The size/view/case loops, filtering and default 21-cell workload are unchanged.

Two completed root check receipts were inspected at this checkpoint. Each records normal counters enabled and unchanged source. Independently hashing **all 322 paths** in each manifest found zero differences from current source, including all ten paths above:

| Completed receipt in `candidate-copy-verified/` | SHA-256 | Completed checks in its actual check list |
| --- | --- | --- |
| `checks-fmt,targeted.json` | `c2609d162a822f2b5f16657c6d3a4b74eb279c16cc90b7027525160accd89b46` | fmt; targeted |
| `checks-fmt,targeted,core-production,workspace,public-integrations,clippy,docs,doctests,dependencies.json` | `20998335140004103497af1556ca0bfdf4e67f7ded030ef1b82663e0579f8eef` | fmt; targeted; core-production; public-integrations |

The inspected logs report **124 targeted passed**, **2058 production all-feature core passed, 3 skipped**, and **4 public integrations passed**. These are root's executed correctness checks. The long receipt filename is not evidence that every named gate has finished: its actual list at this checkpoint contains only the four checks shown. Workspace, Clippy, docs, doctests and dependency completion are not claimed here; workspace.log did not yet contain a completed summary at the read checkpoint.

No normalwork.csv existed in candidate-copy-verified at this review. The ignored work probe is not made executed by the ordinary successful suites, so this augmentation makes no per-cell counter result, physical allocation, memory reduction, elapsed gain or 2× goal claim. The earlier temporary-only status is superseded only for application/typechecking and the completed correctness gates documented above. Performance qualification and the remaining acceptance gates stay open.
