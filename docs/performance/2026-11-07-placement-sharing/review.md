# Independent review: shared placement directory ownership

## Findings

**No blocking source finding.** The frozen standalone patch preserves the existing placement writers and reference/publication rules, shares the directory until an actual write, and detaches every mutable write through Arc COW. The new test uses supported real checked commands and a real prepared-candidate discard. It does not fabricate a late failure or claim execution evidence.

Suitable for root-owned integration and serial qualification of these exact bytes. This is a source review, not a claim that the candidate compiles, its tests pass, or it improves latency, allocations or the twofold objective. No required source correction or expanded framework is requested.

Review completed at `2026-11-06T22:12:01.436448+00:00`. I did not run Cargo, a compiler, tests, benchmarks, timers, network operations, Git writes or subagents, and did not modify the repository, candidate sources, caches or forks. The only written artifact is this report.

## Exact identity and bounded scope

The following identities were independently recomputed:

- Patch: `/tmp/placement-owner-sharing/placement-owner-sharing.patch`, SHA-256 `cfe9f0cb896c2db6aaa23de17d40328ae279a69038a6869698275fa84ec0e3c5`, 7,188 bytes, exactly one source path, +123/-10.
- Base `crates/dockspace/src/graph/authority.rs`: 13,734 bytes, SHA-256 `c4542993266ad497b50cdd30e00bbf6fc656819925d8d52942290b54d7d121e2`.
- Candidate authority source: 18,363 bytes, SHA-256 `cae68a84471cc9fa64f74c7345c3566c8b7a3e8b84f870060b55ab2f4e44164a`.
- Base one-path aggregate: `93924b009b0626b8ab2e6b9b23ad35459875562a9479ddd8456c6bb9dc028b51`.
- Candidate one-path aggregate: `2e4dbf4819aef1ed8c5c258ce2fbaecce59d89d79499d25cc44c258baf3887b4`.
- Manifest SHA-256: `f1148d3bc7a7ed9396121a6520692a12abc72cd1740d0614fe3845d7a4e1e724`; base manifest SHA-256: `2489a3ff2d11a1ca09a6ac8f483cdd40ae2ad5852551743aa4faed4dbc879f57`.

Both source trees contain exactly the one manifest path. Reconstructing the unified diff from the captured complete files reproduces the supplied patch byte for byte. Runtime-only counts independently reproduce +13/-10, net +3; the additional 110 lines are the one 109-line registered test module and separating newline. All artifact sizes/hashes and READY bindings match.

Reversing only the approved field/default/derive changes, two guarded mutations and three mutable build arguments reproduces the complete original runtime file byte for byte. This independently establishes that reference issuance, selection provenance, root completion/install calls and execution rules were not changed incidentally.

The base authority and all eight contract snapshots matched the live sources during review. The captured context is RootStructure + single NodeResult + Tabs-owned MRU WIP. The RootStructure builder still performs full placement installation; the separate root-frontier patch is absent. That other patch's overlapping hunks require explicit composition and separate review rather than treating this receipt as approval of their combination.

## Every mutable placement entrance is covered

`WorkspaceAuthority::Clone` now clones the Arc of the existing SparseSecondaryMap; its roots BTreeMap remains independently cloned. NodePlacement contains only copyable root/parent/index values, with no interior mutable payload. Exactly five `Arc::make_mut` sites exist in the runtime candidate:

| Mutation entrance | Candidate location | COW and retained contract |
| --- | --- | --- |
| One Editing root completion | `authority.rs:147` | Detaches before passing the mutable map to the unchanged complete RootStructure builder. The existing Editing condition remains. |
| Terminal Editing-root completion loop | `authority.rs:162` | Each complete installer receives the mutable COW owner; an already unique map is reused. The existing root order/condition remains. |
| Exact-key retirement | `authority.rs:244` | Checks raw directory `contains_key(node)` before COW, then removes that full key. No live-storage filter is introduced. |
| Actual placement change | `authority.rs:250` | Preserves the live-node guard, compares the entire requested placement before borrowing mutably, then detaches and inserts only on difference/absence. |
| Root birth / initial issuance | `authority.rs:375` | Detaches before complete subtree installation into new/existing candidate authority. Initial Builder/import issuance still uses the full builder. |

The actual `RootStructure::build` contract remains `&mut SparseSecondaryMap<NodeId, NodePlacement>` and still inserts each entered node (`contracts/.../root_structure.rs:92-126`). It has not been converted into a partial installer. Local and cold canonical retirement, runtime removal, subtree unrooting, child-index refresh, subtree assignment, wrapper insertion, root promotion/collapse and birth still route through those owner entrances.

In particular, canonicalization can remove raw arena records before forgetting metadata (`canonical.rs:315-318`, `canonical.rs:416-417`). A check through live-filtered `placement()` would skip that cleanup. The candidate correctly checks the raw map, so retired identities can be removed even when storage no longer contains them. Conversely, `place_node` intentionally retains its existing live-node guard.

## Exact equality and full generational keys

`NodePlacement` now derives PartialEq/Eq over both RootId and `Option<ParentLink>`. ParentLink already derives exact equality and contains the full parent NodeId plus child index (`workspace.rs:17-21`). Root-only equality would have missed same-parent index shifts and subtree parent changes; that weakening is absent. The comparison occurs before `Arc::make_mut`.

I inspected the installed slotmap 1.1.1 implementation rather than treating the map as index-only. Its `get` and `contains_key` match the stored version as well as index, and `remove` requires that same version. `insert` retains its rejection of older-generation overwrite. The candidate uses those checked APIs and performs no manual key reduction. A stale forget cannot remove a newer recycled identity; a current replacement is not skipped because an older generation used the same slot index.

Dependency source inspected: `slotmap-1.1.1/src/sparse_secondary.rs`, SHA-256 `6cf90d1b0087b58f1a59f850895ae52beeecace1768351111ca581e334449c29`. Its SparseSecondaryMap is a derived Clone over a HashMap of versioned slots, so the first COW write clones the existing complete directory. There is no second placement map, generic container wrapper or changed generation policy.

## Sharing, issuance, stage visibility and rollback

The source supports the intended sharing cases for concrete reasons:

- Workspace clone shares only this directory handle. A changed or unchanged Select edits Tabs/MRU and, when required, renews selection issuance without touching placement.
- A single Item center/gap transfer whose source leaf survives changes item membership, not node parent/root membership. Existing final-root placement requests compare equal; local finish has no retired/repaired split and its complete-structure calls remain no-ops for Ready roots. Those requests are retained, so true collapse/promotion still installs its changed placement.
- Genuine Item no-op paths return before detach; unchanged placement requests can remain shared without redefining the outcome or reference behavior.
- Rehome renews presentation/root issuance through its existing owner and leaves node RootId/parent links unchanged. General normalization restores unchanged Ready structures rather than forcing an installation.
- Edge insertion, source collapse, subtree transfer, birth and any actual roster/index change detach at their first real placement write. Existing prefix completion still makes those current placements visible before the next staged capture.

Placement equality is not used as command equality or authorization. All RootReference and selection-provenance code is unchanged. `complete_root_change` still issues a fresh reference even when the requested root placement compares equal; rehome still follows its existing same-carrier rule. `capture_reference` still checks live root, live full NodeId, actual membership and exact issuance (`workspace.rs:763-781`). Value-identical published ABA remains distinct from normalized value equality.

The transaction still clones the candidate and publishes only a successfully completed PreparedTransaction. Every real map write first detaches from all old Workspace/snapshot holders. A discarded or late-failing candidate can therefore drop its own directory without modifying the original. The independently cloned roots owner map also keeps old RootReferences isolated; immutable RootStructure/Split Arcs and owned numeric outputs are not replaced by this placement Arc.

Existing unchanged source coverage includes actual-prefix placement and late normalization rollback (`owner_reference.rs:286`), strict net-zero rehome ABA (`:694`), exact no-op followed by a frozen reference (`:745`), retained structures for nonempty cross-root center moves (`:861`), and projection ownership after source disposal (`:887`). `graph/copy_tests.rs:304` retains the exact late index-1 ItemAlreadyOpen failure, original Workspace/node-pointer/reference checks and a subsequent successful command. These are registered source witnesses, not tests executed by this review. Layout results do not retain this mutable metadata directory, so sharing it introduces no new output aliasing channel.

## The added test uses supported actions

The new nested `#[cfg(test)] mod tests` is registered through the existing `graph::authority` module (`graph.rs:808`). No public test accessor or fake owner mutation is added.

Its initial Horizontal split contains tabs `[1,2]` and `[3,4]`. The test retains an original Workspace, the exact placement Arc and a frozen root reference. It then uses the real checked transaction Interface for:

1. Select item 2 in the first tabs stack.
2. Move that Item to the second stack's Center. The source retains item 1, so this is the supported nonstructural Item route, not an unsupported whole-subtree merge.
3. Rehome the actual root Split to unused surface 2 through `RootPresentationTarget::NewSurface`, an implemented checked rehome branch.

Each report must be changed while retaining the same directory allocation. The original frozen reference still verifies in the original Workspace and is stale in the changed Workspace; original MRU remains `[1,2]`.

The structural half moves Item 3 from the still-populated second stack to the Top edge of the first. The root is Horizontal while Top insertion is Vertical, so the real operation creates a wrapper and changes the first stack's parent. The test prepares this candidate through `prepare_transaction`, verifies directory detachment and actual changed parent link, then drops the candidate without publication. It checks that the live Workspace still shares the original directory, equals its pre-prepare clone and has its original parent link. These conditions are not satisfied by merely incrementing a counter or inventing a synthetic placement change.

This one new test covers changed semantic sharing and prepared-candidate discard; it does **not** directly execute a late-error case, an explicit no-op, every generational reuse, or a retained numeric-output comparison. The README accurately describes that scope. Existing broader tests and the complete COW write inventory supply the source-level argument; root still owns their execution. No blocker was inferred merely because the new narrow test does not duplicate the whole existing suite.

## Costs and qualification limit

No-placement-write candidates can avoid the eager complete sparse-table clone. A first real write to a shared directory still clones the entire table, and the retained full RootStructure installer still forces that fallback even when its final placement values are equal. Weight-only structural rebuild is therefore not claimed allocation-free. This does not make structural candidate ownership O(frontier), remove arena/root/presentation copying or eliminate normalization work.

New costs include the Arc allocation for default/empty authority, atomic handle operations, read indirection and pre-mutation equality/presence probes. The existing `mem::take` follow-up path constructs that default owner. Sharing can change lifetimes, clone timing and peak memory. Existing workspace-storage counters exclude placement-directory copying; unchanged counters cannot establish zero copying or quantify this saving.

Root should qualify the exact standalone bytes before measurement, then explicitly compose and review any root-frontier installer change. This review establishes no performance or allocation result and does not change the frozen complete-operation protocol.

## Captured unchanged context

Every following file matches its saved contract and the live source observed during review:

| Contract source | SHA-256 |
| --- | --- |
| `crates/dockspace/src/graph/root_structure.rs` | `de30fe16e4587efe9cc181f5d1ca46b66250fa082c5b4363efa5f1c9a1215a62` |
| `crates/dockspace/src/graph.rs` | `dde95ddfa9e733448a4565404d9b6a612a3f614735bc39ba2cc4233066b80c2a` |
| `crates/dockspace/src/operation.rs` | `b18c0f29b0561af1bbed80600d72d94825d5fb5ac1708abf1af7fe864d15c0b5` |
| `crates/dockspace/src/canonical.rs` | `b4d5abe7b5916e41efc8004f294aacf0623716a29e94d50a1fc9185a59fa03d4` |
| `crates/dockspace/src/transaction.rs` | `0970cc5343f6eec5bc300603e78303782b3e817edb86064b3694fac4544106a2` |
| `crates/dockspace/src/workspace.rs` | `5cdb89c10b26f95695b9995e5c83e8cc3d2b34029b3213211d608b0c2cea0dd0` |
| `crates/dockspace/src/graph/copy_tests.rs` | `05e0cf43f8e16f4eab0808eab1c0dd67fe00a6ae49ac4ebe06f07175c260fda1` |
| `crates/dockspace/src/operation/tests/owner_reference.rs` | `8a39eb6df3d844f952af7b550f75f51c51ab6d5c853b47f8748b69adb55158d4` |

Review complete; candidate stationary; STOP.
