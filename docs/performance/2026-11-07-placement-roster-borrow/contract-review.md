# Borrowed placement roster: contract review

No source blocker found. The changed refresh loop preserves the previous `place_node` behavior while borrowing the existing child roster instead of cloning it. Existing semantic coverage is adequate for ordinary qualification; no mirrored storage-form test is needed.

Reviewed `post-main-continuation/placement-roster-borrow-source/` without consulting its other review:

| Artifact | SHA-256 |
| --- | --- |
| Base `graph/authority.rs` | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |
| Candidate `graph/authority.rs` | `61245b367348b1fa80cdc1759e4e4686230f6bc83430829899571a72dec05005` |
| `runtime.patch` | `57ec91635b6d9a146259b3f97c7808d5939420dc7d64c94b755661378161f9ee` |

The live file matched the candidate. Only `refresh_child_placements` changes.

## Preserved behavior

- Parent placement is still obtained first through `self.placement(parent)`, including the live node-generation check. Missing placement returns before examining the node. Missing/non-Split parent then returns without modifying placements (`authority.rs:282–289`).
- Children are visited in the same roster order with the same enumerated index. Each child still passes `NodeStore::contains_key`, which delegates to SlotMap using the complete generational key. Missing or stale-generation children are skipped rather than newly rejected or assigned.
- For each live child, the constructed `NodePlacement` has the same root and complete `ParentLink { parent, index }`. Full equality is checked before `Arc::make_mut`. No eager placement-map copy is introduced; empty/all-equal rosters preserve sharing. Duplicate raw child entries would retain the old ordered last-write behavior.
- Borrowing `storage.nodes` immutably and `authority.placements` mutably is safe field separation. Placement-map COW cannot change, reallocate, or invalidate the graph child roster. There is no callback, graph write, new persistent state, or alternate membership rule inside the loop.
- Root issuance, raw-key forgetting, `place_node`'s remaining callers, completion, normalization and transaction publication are unchanged. A changed map detaches from older candidates/snapshots by the same Arc COW mechanism; unchanged maps remain shared.

The three production refresh sites remain split-child removal, insertion into an existing split, and wrapper creation (`operation.rs:3145,3370,3472`). Therefore shifting sibling indices and newly wrapped child links remain covered by the same operation completion paths. The function is infallible as before and introduces no typed-error ordering change.

## Existing meaningful coverage

`graph/authority.rs:461`, `semantic_edits_share_placements_and_discarded_structure_edits_detach_them`, covers sharing for semantic edits, real structure-edit isolation, original-reference survival and candidate discard.

`operation/tests/local_item_move.rs:113` exercises axes, source collapse, same/cross-root moves, before/after and inner/outer edges; it invokes the independent actual-roster placement oracle. `operation/tests/owner_reference.rs:33` constructs that expectation from real root records and child rosters, covering parent indices as well as root membership. Its cross-root/staged-residue case at `464` and recycled-generation case at `514` add staged and generational checks. These are semantic checks rather than assertions that a Vec clone disappeared.

The source deletion is one temporary child-roster Vec clone and its disposal per refresh. Necessary placement COW, child membership checks and Node payload ownership remain. This review establishes no timing or allocator-share result.

Only static source/diff/hash and existing-test inspection were performed. No source, candidate or test was changed; no build, test, timing, runner, profile or network operation was performed.
