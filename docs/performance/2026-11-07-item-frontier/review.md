# Independent Arc composition review

Verdict: no confirmed correctness or scope blocker in this context rebase.
This is a source review and static patch check; it is not executed behavioral
or performance qualification.

Reviewed patch SHA-256:
`70cfba79fba97372f09fc4dcc8ae8ccc7d6e0ae396b922995aac125c4578b10d`.
Original author: `root_frontier_placement`. Original patch SHA-256:
`fc27efcd998edaed8f1b3decf78c1aa38856f80d9bed87c817628be6fdc52c99`.
The original review is bound by SHA-256
`f7f67670517fc56d5fb3289fbee55ad784f0e2c923392ae0a495e74b1e0616fa`.
Exact base and output identities are recorded in `manifest.json`.

- **Restricted completion boundary.** In candidate `canonical.rs:313-335`,
  the existing retired-key cleanup, final root placement, and changed
  direct-child refresh precede both rows-only calls. Numeric input/output
  frontier checks precede that repair and remain unchanged. Only the
  terminal single-Item local normalizer calls the new method. Current
  `operation.rs:791-799` still completes staged Move prefixes through the
  ordinary full installer. Donor, birth, import, and raw/general terminal
  completion paths retain full installation.
- **Arc COW and retired generations.** Current `authority.rs:260-273`
  remains byte-identical: `forget_placement` tests the full raw key before
  `make_mut` without requiring a live arena node; `place_node` compares
  the complete root/parent/index placement before COW. Thus raw arena
  removal followed by cleanup still removes retired entries, and an
  identical root placement can retain sharing. Actual edge insertion,
  shifted sibling indices, and repaired parent links still execute the
  existing concrete writes and detach shared storage when needed.
- **No general rows-only escape.** `authority.rs:136-151`, `170-180`, and
  the birth installer retain all three ordinary `RootStructure::build`
  calls with their existing `Arc::make_mut`. The new `153-167` method only
  completes an Editing owner; absent/retired owners and Ready owners keep
  their existing behavior. It is selected only after Item repair, rather
  than assuming every Editing owner has valid placements.
- **One structural implementation and preserved outputs.** The full and
  rows-only entry points use the same `root_structure.rs:110` build loop.
  The full wrapper inserts at the original Enter location; the Item
  wrapper omits only those repeated inserts. Node reads, DFS order, row
  sorting, shared split payloads, errors, and successful completion
  counters are unchanged. Assignment to Ready follows successful build;
  partial candidate state is not published if a later build fails. Old
  outputs retain their immutable shared bodies and payload ownership.
- **Exact scope preservation.** The patch remains +38/-4. All original
  addition/removal bodies are unchanged; only Arc-bearing context differs.
  All five existing `make_mut` sites, exact root issuance and selection
  provenance, and the complete 4,464-byte semantic test module survive
  unchanged. No test, executor, validator, cache, flag, or dependency was
  added. Real placement writes and whole-root rows construction still
  have their existing non-frontier costs.

The current local-item success matrix compares placements with actual
graph traversal, including empty-source repair, flattening, same-parent
index changes, and same/cross-root moves. Existing f32 frontier witnesses,
staged/donor/residue/ABA cases, and the Arc-sharing semantic test provide
the relevant root-owned execution checks. None was run in this lane.
