# Scalar issuance implementation review

**Findings first: no blocking identity, ABA, selection-admission, rollback, or error-order defect found in the reviewed source. The candidate is suitable for the declared qualification and performance screen; this is not qualification or performance evidence.**

One low-priority opacity caveat: RootReference's custom Debug remains unchanged in behavior, showing only root and a non-exhaustive marker (`candidate/.../graph/authority.rs:48–54`). However, RootIssuanceId derives Debug, and the new RootOwner predecessor field contains that type. Public Workspace derives Debug (`graph.rs:417`), so formatting a selected workspace now exposes the predecessor serial through WorkspaceAuthority/RootOwner. This does not expose a constructor or confer authority. A claim that *all workspace debug output* hides issuance numbers would be inaccurate. If broader redaction is desired, a small opaque Debug implementation for the private ID suffices; no identity machinery needs changing.

Scope: read-only inspection of `post-main-continuation/scalar-issuance-source/{base,candidate,runtime.patch,predeclared.json}`. No Cargo, compilation, tests, workloads, timing, network, or repository writes. The packaged prior assessment and witness plan are design inputs, not independent implementation qualification.

The three base files equal immutable `c7508eec3ac44138305797691244ae9c180d7680`. I applied each unified-diff hunk in memory and verified exact candidate bytes, including context/counts; the three packaged candidates also matched the live files when read. The patch contains only these three paths, +108/-33 including tests. The implementation conclusions below bind the packaged candidate, not a future live tree.

Issuance and identity:

- RootReference stays opaque and Clone, without Copy, serde, a numeric constructor, or exposed serial access. Its root accessor survives workspace destruction. Derived equality compares root plus private issuance, which preserves exact equality because the issuer is unique across all fresh calls within this linked implementation.
- The one static AtomicU64 starts at 1. Successful relaxed `fetch_update` returns a distinct pre-update value and advances through checked addition; concurrent callers cannot obtain the same successful issuance. At the exhausted value, updates fail and `expect` panics without wrapping. The final possible integer need not be issued. There is no reset, recovery pool, per-workspace issuer, or candidate recycling.
- This is the explicitly accepted new fail-stop resource-exhaustion policy, not a claim of preserving an old recoverable exhaustion error. Ordinary CommandError/Result signatures and precedence remain unchanged. Existing project use of AtomicU64 means atomic width is not a newly introduced platform requirement.

Every writer/restore site is covered in `authority.rs`: ordinary root change replaces the reference and clears predecessor, including the missing-owner construction; root birth clears predecessor after the original collision/structure checks; selection records exactly the old issuance and full NodeId before minting a new reference; actual presentation change clears predecessor, while same-carrier rehome leaves both fields untouched. Root removal still removes its owner. Initial storage issuance reaches the existing root-birth path. Structure-only maintenance does not mint or rewrite predecessor metadata. Derived owner/workspace cloning copies identity and predecessor together without minting.

`retain_original_issuance` restores both fields from the same original owner, preserving its old loop/missing-root behavior. Its single-Moved durable-no-change invocation in unchanged `operation.rs:823–828` is not broadened to general net-zero batches. Discarded/failed candidates burn identities and cannot make predicted-branch references valid in another candidate. Full-generational membership and stale-node verification are unchanged; no key decomposition or serial substitution for NodeId occurs.

The only production selection consumer still runs after epoch, roster, unrelated-root reference, membership, node-kind, and current-reference lookup checks in `WorkspaceIndex::after_selection`. Its fallback queries the current immutable workspace owner for the exact prior issuance and Tabs key. The added previous-root comparison is consistent with the existing indexed-root invariant. Exact current-reference equality still bypasses the predecessor fallback. This accepts a direct selection transition without admitting arbitrary ancestry, a foreign build, another root, or another Tabs leaf. The workspace borrow prevents the owner changing between current-reference lookup and the query.

The tests have complementary roles. The extended existing owner-reference test executes a real single Tabs self-center Move through general transaction finish, checks unchanged durable state/reference, then checks wrong-Tabs rejection and successful original-index selection rebinding. A real second Select on a cloned branch rejects the original index while admitting the intermediate rebound. Existing foreign-build and intervening-edit checks remain. The no-op Move does not first mint an identity, so it is correctly only the preservation witness.

The additional authority test fills that limitation: it establishes predecessor metadata through a real Select, then explicitly performs a private candidate root edit, verifies the candidate identity differs and predecessor admission is cleared, restores original issuance, and checks both identity and predecessor admission return. The original still admits its predecessor. This is a narrow private restoration witness supplementing real transactions; it must not be described as an entirely public transaction scenario. No counter-mirror framework or new fixture machinery was added. Source inspection supports these assertions; I did not execute them.

Cost boundaries remain as predeclared: removing Arc issuance allocations/refcounting trades for wider references/owners and a global contended atomic on real edits. Embedded commands, targets, indexes, errors, and owner-map copies may grow. No exact aggregate sizes, allocator share, or speedup is established by this review. Solvers, reference oracle, fresh inputs, full outputs, work counters, and timers are outside the three-path patch and unchanged.

SHA256 bindings:

| Artifact | SHA256 |
| --- | --- |
| `runtime.patch` | `7cd1327a27eed343e475e3435df95de2befb837a4b6a6cc51d9ca91bea18f728` |
| `predeclared.json` | `b35a5b165450776587da431ebbffcb82715ea629309fcf6d12ad2040325b66b4` |
| base `graph/authority.rs` | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |
| candidate `graph/authority.rs` | `b6557067afd9ca5066bce7c5fbe6b1b5a203402f16293814938ca7374df3f7d5` |
| base `operation/tests/owner_reference.rs` | `0d6a8d0f5931976a1561790a48d53273c8f68e5983d6ad25c8ff7989d4aa7199` |
| candidate `operation/tests/owner_reference.rs` | `259daa1781ed31dd36f50f43e0bc23c5fe774a494298228c43f2207445c89ab9` |
| base `workspace.rs` | `6cbcca776bfca60b1e767dc909121f06ae751110ea1f9fd265ea398da644aa36` |
| candidate `workspace.rs` | `e8b3cf88b5fb65c3ff2f9ea8a7947a458e67d5f833c3665ec3acab63c70de6f0` |

Source assessment input SHA256: `bbbf960bd0ce4869ac1f52d918361c23c74d84f55e057f0e567ab758e57af285`. Witness-design input SHA256: `3e020cdb7c83c3fb92b213f2543c323937c3930b38d9dace5c3239b0184ecb46`.
