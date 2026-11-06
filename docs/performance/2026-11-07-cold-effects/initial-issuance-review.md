# Source review: initial root issuance from one carrier discovery

Date: 2026-11-07. Verdict: **no blocking source defect found** in this exact initial-issuance slice. Root can apply it after its stated prepared-report/copy/combined-cold dependencies and run the normal qualification gates. It remains uncompiled, untested and untimed in the reviewed artifacts.

This reviewer did not author initial-issuance. The reviewer authored the precursor cold-effects candidate; this review is independent of the new discovery implementation, not a claim of independent review of that entire prerequisite chain. No repository edit, Cargo/compiler/test/benchmark/timing execution or Git mutation occurred. Exact patch replay used only in-memory text and hashes.

## Exact subject

| Artifact | Independently calculated SHA-256 |
| --- | --- |
| `initial-issuance.patch` | `0c18795d42f09e9fce35fec22732f9a37c2ec1939dd9c96fec08ad65c54d7176` |
| `runtime.patch` | `615423286900b0c7eb7ba8645e75f742cb626cc252d6abe30f066f0a55ccbdac` |
| `README.md` | `e027fa7054c433cd8614b9fe8d3621ccb3097e592077b486bddc18dc8475c9e4` |
| `receipt.json` | `2ebd0a7abb0048ce028aed58ce69654d43945957112f3e6ed863b98af12b87c7` |
| two input files, aggregate | `174f44e7a737575093ddd0e536d20ec59e95fc7d00a43ccc16769cd2e25b06e0` |
| two output files, aggregate | `f9d5a81c709b455cf9b41831d7bc8b48ed2cda463d08abc01d26e1f1b8aa9fae` |

Aggregates use sorted repository-relative path bytes, NUL, then complete file bytes. Each base equals its exact cold-effects-combined proposed dependency, every patch hunk was replayed against exact context, and both results match their saved proposed bytes and manifests. All referenced dependency hashes and the complete/runtime/test patch hashes match receipt.json.

| Path under `crates/dockspace/src/` | Input SHA-256 | Output SHA-256 |
| --- | --- | --- |
| `graph/authority.rs` | `6686d5767ec12dbd4e0ce9c39c6ccb78412b4f742a95ade18c39977dad75b467` | `b1a71a5ca26202f94dd46a6b47676225c22e9d08769a990b3b7c737b28ed4f2a` |
| `operation/tests/owner_reference.rs` | `22f13bcb417d45bca9c8d9126cd95c8d97e84076c068443de0789814a26582f1` | `4a170b0e6d0f3bc48d888a061db1065795a77d7eaf8f46df802c8d29d20ef869` |

The production delta is +29/−27 in one file. The second file appends two registered tests, +83/−0. No live repository file is assumed already to equal these future combined-cold inputs.

## Carrier discovery and stable root order

The previous per-root presentation_in_storage scan visits surfaces in BTreeMap order, preferring a matching main root before contained entries, then visiting each contained roster in order. The new discovery visits those exact same storage records in the same order and records their actual Main/Contained values. Missing floating records do not create a pair, just as the old search skipped them.

sort_by_key is stable. Consequently equal-root pairs retain discovery order, and dedup_by_key retains their first carrier. This reproduces the old first-match preference for private malformed input rather than choosing the last record. The resulting distinct root list is ascending by the same RootId ordering used by the old storage.roots.keys loop. complete_root_birth is called in that order; root-map iteration after construction alone would not prove issuance order, but the actual sorted consume loop does.

Each pushed key passes actual-root membership. After first-entry dedup, every retained key is a distinct member of storage.roots. Therefore the equality of pair count and actual-root count proves complete root coverage without assuming that an unrelated foreign presentation substitutes for a missing root. Foreign main/contained root IDs are filtered exactly as the old loop ignored them while searching for each actual root. There is no new authority entry for a foreign root.

A missing carrier still returns `CommandError::Invariant { stage: "refresh root presentation authority" }`. It now returns before any initial subtree assignment instead of after an earlier root's private partial assignment. That partially built value was never published, so the observable typed failure is unchanged. Given filtering, default-empty authority and deduplication, the new birth loop cannot normally produce MissingRoot or the duplicate-birth invariant. It adds no new public malformed-input acceptance.

## Full validation remains before initial issuance

In the reviewed combined-cold tree, issue_storage's only caller is canonical::build_workspace. That caller first runs canonicalize_data, including structural input diagnostics, rewriting, before/after item reconciliation and complete strict output validation, then maps an issuance error to the existing AuthorityCompletion wrapper. Graph's Builder and import/restore use that same path. Default empty Workspace creation has no roots to issue.

Thus the discovered carriers come from completed canonical storage in production. Duplicate presentations, missing main/contained roots, absent floating records, empty surfaces, unpresented roots and malformed rooted membership retain their original deterministic validator collection before this new code runs. No mutable or not-yet-normalized candidate is exposed through a new Interface.

The old refresh_root_placement has no other caller after the combined-cold changes, so deleting it removes a genuinely unused repeated search path. presentation_in_storage remains intact for real fingerprint and diagnostic/oracle consumers, including complete_root_change's fallback. This slice does not remove current carrier checks from those paths.

## Exact refs and normative placement

complete_root_birth reads each actual root record, rejects an already issued root, assigns the actual subtree and inserts a RootOwner with the supplied discovered carrier and a fresh RootReference Arc. It performs no hidden roster search. Starting from the default-empty authority table and invoking it once per distinct root establishes all rooted membership and actual parent/index placements, including non-leaf roots. place_node still checks complete live NodeIds; no highest-slot table or lossy key is introduced.

RootReference::fresh and immutable Workspace Clone are unchanged. Independent Builder/import completion mints one fresh root issuance even when durable values equal an older Workspace; immutable clones retain their existing refs. Placement assignment and issuance happen only in the private new Workspace, so failure cannot alter an existing issued original. This does not relax strict ABA or source/target checks.

Empty validated storage produces no pairs and no authority/placement entries. It returns the same valid empty Workspace value, with MissingRoot capture for a foreign root. Roots whose main/contained location differs from RootId order still receive their actual discovered carrier; sorting changes only issuance order, not the carrier paired with the root.

## Tests and cost claims

The mixed-carrier test deliberately collects root IDs in surface order C, B, A, extra, unlike ascending RootId order. Real Builder completion is run twice; durable values remain equal, every independent rebuilt ref is fresh/stale against the old root source, and the existing actual-placement oracle independently derives membership, parent indices, paths and raw carrier lookup. The test exercises a split root plus leaf roots and main, contained and rootless-contained-only surfaces.

The second test retains the empty Builder case and five canonical-graph presentation corruptions. Builder::validate obtains the complete typed/ordered strict error collection; build must return the identical InvalidOutput collection. Its foreign root, missing floating record, duplicate carrier, missing carrier/empty surface and foreign contained-root cases fail before initial issuance. This is appropriate public gate coverage; no unchecked publishing seam was added just to test a private malformed path.

These tests have not run. The existing clone/independent-build/published-ABA, full placement/index, strict presentation/backlink and persistence/import suites remain necessary root gates. No existing assertion is weakened or deleted by this slice.

The source does remove R repeated carrier scans and replaces the old root Vec with a pair Vec plus stable sorting. Discovery still performs actual-root/contained BTreeMap lookups, sorting uses O(R log R) work and O(R) possible temporary storage, and birth still performs subtree/directory/authority work. The README states these costs and does not claim allocation-free or strictly linear whole-Builder execution. Builder fixture construction remains outside the primary docking-kernel timer; there is no primary-kernel gain to attribute from this source change alone. No source/runtime blocker or required extra abstraction was found.
