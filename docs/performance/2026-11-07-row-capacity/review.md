# Independent source review: root row capacity

No source blocker was found in the two-file candidate. The new Editing payload is a private allocation hint, and the candidate preserves the parent Item rows-only completion boundary and full placement installation on the other paths. This verdict covers source behavior and ownership, not successful compilation, behavior verification, memory layout or performance. Root remains the sole verification owner.

The review binds these exact complete source pairs:

| Source | Base SHA-256 | Candidate SHA-256 |
| --- | --- | --- |
| `crates/dockspace/src/graph/authority.rs` | `89c65b0661ae3cc31b38b0157d4767e77c68219186ce78ca784ffb44d6af3fb9` | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |
| `crates/dockspace/src/graph/root_structure.rs` | `a4746315ca204bad7759db02eaf9d95488b6169ec873dab6207766d669b8e710` | `e80a74d1bdef525bd25a9a627bd1c826f41cd1def8fe05583dcea14372938632` |

`candidate.patch` SHA-256 is `13a67cd74cddc850dbd8064725ddd90bc7d57efbeb4eb93efe79575aff5d78e6` (+42/-13). Both base files equal the exact `/tmp/root-frontier-placement-current/tree/` files. The parent patch (`70cfba79fba97372f09fc4dcc8ae8ccc7d6e0ae396b922995aac125c4578b10d`) and parent manifest hashes match. Independently generated diffs from the complete pairs reproduce the candidate patch exactly; manifest, READY, artifact and path-set hashes match. No patch was applied or executed.

The state transitions retain the original authority behavior (candidate authority.rs:101-189, 328-408):

| Existing state or operation | Candidate behavior | Authority consequence |
| --- | --- | --- |
| Ready, first `begin_root_edit` | Read only `nodes.len()`, then replace Ready with Editing | The owner's old Arc is dropped as before; unfinished roots still have no structure read view |
| Already Editing, repeated begin | Preserve `previous_nodes` | No earlier body is restored or treated as current |
| Absent owner, begin or complete | Existing no-op behavior | A hint cannot create an owner or authorize a root |
| `take_root_structures` on Ready | Move the existing Arc into the prior-body list; install Editing with zero | Same raw-normalizer ownership transfer, without an additional Arc clone |
| `take_root_structures` on Editing, including a repeated take | Clear any hint to zero; return no body for that owner | Safe loss of an optimization hint; no stale body can be restored from this call |
| `restore_root_structure` | Install the supplied Ready Arc only if the owner still exists | Unchanged normalizer contract; absent/retired owners are not resurrected |
| Item completion on Editing | Build fresh rows using the scalar; install Ready only after success | Placement repair remains the parent's responsibility; the count never authorizes placement |
| General/full completion on Editing | Use the original full builder with zero hint | Complete installation of actual placements remains intact |
| Fresh owner introduced by `complete_root_change` | Editing with zero | No prior body is invented |
| `complete_root_birth` | Unchanged root-presence/collision checks and full build before insertion | Birth never uses an old-owner hint |

Ready completion remains a no-op. Existing-owner `complete_root_change` still changes issuance/presentation without converting Editing back to Ready. Normalizer restoration remains restricted to roots whose actual rewrite effects are unchanged; owners that were Editing before the take were never captured. A failed builder leaves the existing Editing state uncompleted. These are the parent's discardable-candidate and restoration contracts, with only the scalar added.

The hint has no authority use. `root_structure` still returns `None` for every Editing value. Exact root references, selection provenance, generational placements, presentation checks and the unchanged authority test module do not consult `previous_nodes`. Its only value consumer is `build_after_item_move`; the shared builder uses the resulting value solely in `reserve` calls. The hint does not feed a length, index, iteration bound, callback, validation result or publication condition.

The capacity arithmetic is sound for this caller. The unchanged single-Item transaction branch is limited to one checked Move. Its edge insertion creates at most one tabs leaf and one split wrapper; same-axis insertion creates only the leaf. Source cleanup and local flattening remove nodes or reuse identities rather than creating additional nodes. Thus a surviving affected root has at most `oldN + 2` rows. Separate source roots cannot grow through this operation, removed owners skip completion, and roots with unchanged topology retain Ready without a rebuild. Repeated begin calls during target wrapping and source repair preserve the count of the last Ready body. General multi-command prefixes use full completion instead of consuming this single-Item estimate.

For a canonical rooted tree with N nodes, every node except the root has one incoming child edge, so E = N - 1. Every split has at least two children, so 2S <= E and S <= floor((N - 1) / 2). Reserving from h = `oldN.saturating_add(2)` therefore covers the node directories, child edges and split rows for that checked completion. Zero deliberately disables the estimate. These facts establish reservation sizes only: the existing pushes, resize, pending stack and traversal all remain free to grow beyond capacity. The hint is not a replacement tree validator or bounded traversal. No global arena inventory, maximum key, or sibling-root count is read.

Actual final node kind controls reservation (candidate root_structure.rs:140-161). A final Tabs root reserves only the three one-element node directories. It does not reserve children or split_rows; the unchanged leaf traversal and empty filtered directory rebuild do not fill either split array. A final Split root requests h for nodes/postorder/ordered_rows, h - 1 for children, and floor((h - 1) / 2) for split_rows. These are element capacity requests, not initialized lengths or exact allocator byte sizes. Vec may reserve more than requested. The pending stack and the final RootStructure Arc allocation remain on both paths.

All original root-record and node-presence checks, callback order, child traversal, split-weight debug assertion, payload reads and completion counters are byte-identical after removing only the inserted reserve block. Directory sorting and row lookup are byte-identical. The reserve block follows the original first-node presence check. The parent numeric-frontier checks, flattening, retired-placement cleanup and direct-child refresh still precede rows completion; canonical.rs is not part of this patch. Ordinary returned validation errors keep their original relative order. Earlier reservation can change memory pressure or the point of allocator failure; this is not an allocation-failure-order equivalence claim.

There is no added RootStructure or split-payload Arc owner. `begin_root_edit` borrows the old body only to copy its count, then drops that owner as before; Editing stores no body. Raw take/restore moves exactly the existing Arcs. Build creates the same final RootStructure Arc and split payload clones as the parent. Existing output/workspace owners and any placement first-write COW remain, but the hint adds no new shared body that can itself force an additional node COW.

Costs remain material and should decide retention through root-owned measurements:

- The previous Ready-Arc/unit-Editing enum may have used the Arc null niche. A usize-carrying Editing variant may enlarge RootStructureState and RootOwner even while Ready. No exact size or niche preservation is established. Every whole authority BTreeMap clone can consequently copy larger values; many-root and small-root paths can pay this cost without using the hint.
- First begin on any Ready root now reads its old node count, including non-Item writers that ignore the hint. General/full/birth/raw builders still pass zero, but the shared builder has an additional conditional; whether optimization removes it on those paths is uninspected.
- An Item rebuild can replace growth reallocations with earlier reservations of the same five vectors. It still performs a complete row build, directory sort and Arc creation. A final Split root that shrinks can retain substantial unused capacity in its published immutable body. The final-leaf branch avoids reserving split buffers from the old size but is not allocation-free.
- Pending-stack growth, graph payload work, placement COW and all non-Item completion work remain. Reservation requests alone establish no allocator share, latency saving or overall source-level speedup.

Only this review file was written. Candidate source, patch, README, manifests and READY remain stationary. No Cargo, compilation, tests, timing, runner, repository/fork/cache writes or sub-agents were used. Existing parent behavior/oracle gates remain required; this review supplies no executed-pass claim.
