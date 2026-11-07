# Tabs insertion COW source review

No source-level blocker found in the exact frozen candidate. The implementation follows the narrow ownership proposal: capacity-aware copying only for an insertion into shared, nonempty Tabs; unchanged generic mutation elsewhere; the original insertion/MRU algorithm remains single. The added Open followup is a real operation through the existing owned-draft entry point and checks the intended unique-node case. This review permits qualification, not a claim that qualification passed or performance improved.

Reviewed package: `/tmp/dockspace-imgui-twofold-20261107-ea4161e/post-main-continuation/tabs-insertion-cow-source/`. I reconstructed all three candidate files from `base/` and the patch in memory and matched every output byte. All three bases match immutable `c7508eec3ac44138305797691244ae9c180d7680`. The patch contains exactly three hunks: shared storage +33 lines, operation +1/−1, and the existing copy test +29 lines; runtime is +34/−1. No layout or source-nonempty candidate is included.

| Artifact | SHA-256 |
| --- | --- |
| `runtime-and-test.patch` | `8b80e03f89d2675e84daf54a4df6f488900020fed8136b8341d298f65cd75780` |
| `predeclared.json` | `476ba9d1d992b03bb5f6858de167aa63bde9f4c206a3837fdac9bdd34c2d9eeb` |
| Base `graph/shared_storage.rs` | `18c6f75150643779acb9c71c30486634b19f7ccec5677e4cc64ce9043fcc532e` |
| Candidate `graph/shared_storage.rs` | `d3842fe29547ba4f8ef28976d68c5b64a96ea325db12e869de3ff4a9c864e529` |
| Base `operation.rs` | `7586ec06b29b77a794133e0c554d4fcca7a6faa16b8c550b86034ad394175862` |
| Candidate `operation.rs` | `978f506cdbbf5906a7a7b93d596c6779b447cda243697538ab867ad87cf9da23` |
| Base `graph/copy_tests.rs` | `fa5478d74b3e851fa74f26dfeb6d0f1bd2176b091ceca0bd3048697cc1be39c4` |
| Candidate `graph/copy_tests.rs` | `887148c326ab3c86ad9350724e28f6a0b2d3aac09eeb68fe2f07b11c989a08ab` |

Paths and line numbers below refer to `candidate/crates/dockspace/src/`. I performed no build, test, execution, benchmark, timing or network operation, and changed no repository or candidate file.

**Lookup, errors and ownership.** `get_mut_for_tab_insert` (`graph/shared_storage.rs:40–69`) starts with the original `self.slots.get_mut(node)?`. It neither decodes a key nor inserts/removes/recycles a slot, preserving full-generational lookup and allocator semantics.

The specialized arm requires nonzero additional count, observed strong count greater than one, actual Tabs, nonempty items and `index <= items.len()`. Otherwise it finishes with the original `Arc::make_mut`. This preserves the caller's error order: missing target first, wrong node kind next, then out-of-bounds index. Invalid-kind/index requests use the old mutation/copy path before the original error; no new command error or speculative capacity error precedes a known invalid index.

Unique nodes retain their original allocation capacities. A sole strong owner with weak references also takes standard `Arc::make_mut`, preserving its weak-disassociation behavior. On the shared arm, a new Arc is installed only in the candidate slot; all other strong and weak handles still refer to the old allocation. The replacement is built before assignment, with unchanged item order, selection and MRU bytes. Final `Arc::make_mut` has no extra payload clone on the newly installed, unexposed Arc.

`strong_count` is a path-selection observation, not a proof used for unchecked mutation. If another owner drops after that observation, the method may do an unnecessary safe copy; if another owner becomes available before fallback, standard `Arc::make_mut` still handles sharing. There is no project unsafe or race-dependent bypass of COW.

`saturating_add` requests enough capacity for all representable sums. An impossible capacity retains Vec's allocation/capacity-failure behavior rather than wrapping to an undersized successful buffer. Capacity-failure timing can change on valid insertion, which is inherent to this optimization; no recoverable command-error contract changes.

**Original writer and MRU contract.** The sole operation change passes the existing `tabs`, `index` and `inserted.len()` into the helper. Everything after the initial mutable access is byte-identical: bounds check, splice, selected item update, empty-target source-history replacement, MRU promotion and incoming-history append.

The empty-target exclusion correctly avoids preallocating a history buffer that the whole-Tabs insertion arm immediately replaces with `source_mru.to_vec()`. Zero insertions also keep the old path. This method does not introduce membership or MRU validation, reorder a source operation, change the Node arm, or modify command/outcome ownership.

Root issuance, policy admission, placement, structural completion and transaction publication are outside the diff. The candidate still holds its own SlotMap values; original workspace payloads remain owned by their old Arcs. Missing/stale identities, staged inspection and late rollback therefore retain their existing owner protocol. No live undo or mutation of the original prepared workspace is introduced.

**Concrete copy accounting.** At `graph/shared_storage.rs:57–58`, the new physical Tabs copy invokes the same test-only `record_node_payload_clone` once on the original Node, before copying either old slice. The new Arc then reaches `Arc::make_mut` without invoking `Node::clone` again. Fallback relies on the existing Clone instrumentation, so it is not double-counted.

This preserves the meaning of the existing counters: one payload copy and the old lengths of its visual/MRU vectors, excluding inserted elements and allocator events. The existing shared center witness still asserts two payload clones, four tab-item copies, four MRU copies, zero split payload copies, and unchanged NodeStore fork/handle work. Manual recording describes a concrete copy here; it does not report an allocator saving.

**Added semantic witness.** `graph/copy_tests.rs:196–223` extends the original test after its unique Select. It captures a current target, resets counters, and applies a real `Open { item: 99 }` through `apply_owned_draft_command`. It does not create a new candidate clone or use a fake unsupported command.

The expected sequence is consistent with the unchanged writer: the initial center move leaves visual `[3,4,2]` and MRU `[2,3,4]`; Select 3 changes MRU to `[3,2,4]`; opening 99 appends the visual item, selects it and produces MRU `[99,3,2,4]`. The test asserts that entire candidate Node, zero store/payload/item/MRU copies for the unique insertion, the old workspace's unchanged MRU, and validation of both workspaces. Original sharing, stale/unaffected reference and initial copy assertions are retained. It avoids testing capacity or claiming allocation counts.

Existing late-batch rollback and slot-reuse tests are unchanged, as are the broader local-item normalization/MRU cases. There is no source reason to add a new generalized test mechanism.

**Retention limits.** This still allocates a replacement Arc and two Vecs, copies both old slices, and performs the original splice/MRU shifts. It adds eligibility/strong-count checks, and its exact `len + incoming` capacity can leave less headroom for subsequent staged insertions than the old geometric growth. The unique followup correctly allows ordinary Vec growth without claiming it disappears.

These known tradeoffs match the predeclared risk. The source remains a bounded hypothesis. Root's ordinary qualification, work/vector checks and predeclared complete-operation comparison must decide retention; this review supplies no execution or speedup result.
