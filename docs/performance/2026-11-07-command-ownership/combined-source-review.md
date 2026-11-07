# Combined command ownership: source composition review

**No source composition or interaction blocker was found.** The frozen patch is exactly the union of the four reviewed slices: 26 unchanged original hunk bodies and contexts, eight paths, six runtime paths and two test paths, +144/-57 overall and +83/-55 in runtime sources. The combination is suitable for Root's integration and qualification gates. This does not establish compilation, executed behavior, allocation savings or performance.

I authored the single-item slice. Its semantic approval is supplied by the independent `root_frontier` review snapshotted under `inputs/single-item-ownership/review.md`, SHA-256 `d941630e5b41ef6053a8296586428a0dcde637d10374b7e9da10630472487389`. This review does not replace that review with author self-approval. I checked its exact inclusion and interactions with the other three changes.

Only this report was written. No Cargo, compiler, tests, formatting, timing, runner, network, delegation, patch execution, repository/fork/cache write or candidate-source change was performed.

**Exact binding.**

| Artifact | SHA-256 |
| --- | --- |
| `command-ownership-combined.patch` | `621a4aa03bc5103210d139f8a358e9f9177722fdf48fcc3ec19377ed07350e1c` |
| `manifest.json` | `2e182e5bcd4fa124b985ac2235652bb48ffdd22ac5861fefb7a04a00dc4972a0` |
| `base-manifest.json` | `f33fe9efd7c5d8faf288895aa326ce4f79b2b77d5255cb7891bcc0a196d869f0` |
| `composition-receipt.json` | `ba1d6e93d0d7f174b2fcb7f0dbc5de493ff69ae7f4bd45fafc6289f9545c37a5` |
| `scope-checks.json` | `45b6aeb9cfff0ded23625cf4e48970a3a29b17aad4993bef076cb856e1585588` |
| Eight-file base set | `e797ecf9c4e593f37ee82331e814d744dd894a6e231e3c59ca66fc004f27c1a1` |
| Eight-file tree set | `c00f61c40a79066b784954cb3ff5a301803a610715abb31b2788ce06de1163a9` |

Path-set hashing is ascending UTF-8 relative path + NUL + file SHA-256 + newline. All eight complete source-pair hashes and byte lengths match the manifest; base/tree contain exactly those eight files. Artifact and READY bindings match. All eight base files independently match immutable `f00442201db8206857a91d39ebe906a0b410c76d`; all six runtime bases also match immutable `dc689a6`. This review relies on those frozen sources, not later live edits.

**Independent reconstruction.** I read all four copied source reviews and checked their recorded hashes, copied patch/manifest/READY identities and the original review files' hashes:

| Slice | Patch SHA-256 | Review SHA-256 | Hunks |
| --- | --- | --- | ---: |
| Root-record sharing | `239782753c25a34a83e4eda483f0f495f9ecadeed5e28c0d1e1d4bbb498f726e` | `6724a0d2ea4742d6be6c942373bdd937569ca857b7a254cea4b8b7518a8687b3` | 17 |
| Borrowed verification | `be1ebc96b46d0b018daf4994df967f593dbab81521941ab71a70b1ebf4f9be57` | `5d06933df4ae09c24ae0330c291385c247d22edce893a46228b63c33745d17e7` | 3 |
| Single-target policy | `cda0e49ebc069d89889db3074a7991d04ff5b745369953d99641297b441594dc` | `c1d28e0fb738f5211bd29c434093ba2fdc4a945adb021071a80794f559c6cf50` | 3 |
| Single-item ownership | `dfa2f5af3c198cc5e9152c6fc4b1422b7dce01a61c7cef8ae467c3ae9e3305c1` | `d941630e5b41ef6053a8296586428a0dcde637d10374b7e9da10630472487389` | 3 |

Starting from the combined base, I independently applied the original hunks in memory in that order, requiring exactly one complete old-body match per hunk. Every recorded per-hunk and per-stage before/after source hash matches. The result equals all eight complete tree files.

Separately, I reconstructed the consolidated patch at its declared original line positions. It also equals the complete tree. The multiset of `(path, complete hunk body)` pairs equals the four original patches' union exactly. Line-number offsets changed where earlier slices shifted later statements; no added/deleted line or context body was adapted. This independently verifies the composition receipt rather than relying on its pass flags.

The changed paths are exactly `canonical.rs`, `graph.rs`, `graph/copy_tests.rs`, `layout/topology_tests.rs`, `operation.rs`, `policy.rs`, `validation.rs` and `workspace.rs`, under `crates/dockspace/src/`. No authority, allocator, dependency or layout-runtime path is included.

**Root records and borrowed issuance remain separate owners.** `WorkspaceData.roots` becomes an Arc of the durable RootRecord map (`tree/graph.rs:394–401`). Its equality remains value equality (`403–425`). Builder's equal setter returns the original record while avoiding COW (`678–683`); canonical publication compares the actual current root-node identity while retaining changed-subtree effects. The nine writer migrations and their original error phases are exactly the previously reviewed root-record hunks.

The borrowed verifier does not use this map's allocation identity or RootRecord equality as authority. Its ordered checks remain root existence, complete-generation node existence, actual root membership, then issued reference (`tree/workspace.rs:772–790`). An immutable borrow prevents candidate mutation during verification. The returned reference borrows the authority owner, not the shared record table.

The unchanged authority implementation from immutable f004422 hashes to `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600`. RootReference comparison still delegates to exact Arc issuance identity. MRU/selection and actual changed-root issuance do not depend on whether durable RootRecord values changed or their table stayed shared. Consequently, equal records cannot admit stale MRU references, a recycled generation, an independent build or an ABA command.

Successful verification compares borrowed references (`workspace.rs:750–751`). Capture still returns an owned clone (`763–769`), while stale errors own both expected and actual (`754–759`). Old diagnostic/capture lifetimes therefore remain independent of the Workspace. Source verification still cannot lend authority to a distinct target, even in the same root; target recapture and comparison are outside the changed verifier.

**The two policy reductions compose at different boundaries.** A known-target Move still checks source, independent target and move semantics before the optional policy branch (`tree/operation.rs:1201–1218`). That branch builds one inventory with the actual undocking bit, then enters the shared `authorize_move_target`. TabGap authorization remains before the existing drop evaluator (`1243–1272`).

Graph-derived facts still come from `DockPayloadPolicyFacts::new` after actual source-owner derivation (`1363–1388`). The single-target patch's removed constructor source invariant remains unreachable on this same closed call chain; the singleton duplicate shortcut does not change facts construction or permit source-None facts to enter it.

The singleton shortcut returns only from `validate_payload_items` (`tree/policy.rs:2083–2097`). Arrangement/operation/central checks still precede source evaluation, and source/root/surface/item, undocking, class and target rules still follow. Empty non-Root rejection, empty Root acceptance and the ordered duplicate loop for length >=2 remain unchanged. Thus one needed inventory and no singleton duplicate set do not mean zero policy validation or zero policy ownership.

Reusable multi-target eligibility retains its two inventories, constructor invariant and indexed source/target checks (`operation.rs:1063–1139`). Its dispatch uses the same helper. A singleton multi-target query can also avoid the duplicate set, but its meaning and both retained facts owners remain; do not describe the entire multi-target implementation as byte-identical. Policy-None recovery still runs source/target/semantic checks and skips the same optional policy branch. It does not accidentally construct the new single-target inventory or enter the dispatch helper.

**Insertion and root-table COW do not conflict.** The independently reviewed Item merge still calls the original `insert_items`; Node merging and the shared insertion function are unchanged. The Item edge shortcut only places its newly created Tabs through the existing setter (`operation.rs:3299–3309`). Root-record mutation occurs later only when actual root wrapping/removal requires it. Root wrapping retains MissingRoot and NodeOutsideRoot before COW (`3455–3469`); child wrapping updates its original parent slot. Neither path replaces candidate-local placement COW, root issuance, final frontier repair or structural completion with shared-map identity.

I separately compared the complete prepared/report/snapshot prefix, preparation/general/recovery/follow-up span, `move_payload` executor and `insert_items` body with the combined base: all are byte-identical. Owned Moved item lists and ordered TransactionReport outcomes remain intact. The combination adds no fact/proof cache across mutation or staged prefixes. A late extraction, canonical or host-barrier failure still discards an independent candidate; old Workspace references, structures and outputs retain their owners.

**Test and parent boundary.** Root-record sharing contributes one new registered sharing/discard test and one existing raw-test COW migration, not two new test functions. The graph-copy test bytes equal its reviewed candidate. Restoring only the original raw central-write statement reproduces the entire topology base module exactly. Thus the parent's existing compact-frame witness, other geometry/ownership assertions and all remaining topology tests are retained byte-for-byte. No binary, childrows, slab or unified-value implementation hunk is introduced by this patch.

These observations do not substitute for executing existing owner-reference/strict-ABA, actual staged-prefix and late rollback, single-/multi-item policy, TabGap/no-op, central/last-item, Item-versus-Node, Builder/import/serde, recovery/close and old-output suites. The individual source reviews are frozen evidence, not combined executed receipts.

**Costs and reporting limits.** The README correctly retains the negative boundaries: the new empty roots Arc also affects default/mem::take construction; real root-table writes still COW the map; successful verifier savings are temporary Arc clone/drop pairs, not new issuance allocations; required policy/report inventories and durable Node/MRU ownership remain. Singleton branches, Arc reference operations, lookup/code-layout changes and error-path ownership can offset source-level savings.

Ordinary ResizeSplits validation can use the verifier, whereas the formal bounds-only resize request does not fork a Workspace. Keep that distinction. NodeStore handle copies, authority-map copies, placement writes, fresh complete layout and owned export remain.

Future comparison can establish only the qualified combination's overall behavior and performance unless Root separately measures individual controls. The four source deltas do not support additive latency, per-slice gain, allocator-time share, zero-allocation or achieved-target claims. Root's later integrated whole-workspace receipt and complete comparisons remain necessary; this package binds only eight selected files.
