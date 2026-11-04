# Hit inventory compilation

Date: 2026-11-05. Status: complete. Base: `c62d757`.
Implementation: `d7d8b65`.

## Outcome and constraints

Reduce remaining complete-update construction work using fresh CPU attribution,
keeping the existing ordered recording, geometry, semantic ordering and retained
output workload. Preserve exact output authority, atomic publication, native
first-live ownership and the existing graph and presentation ledger. Preserve
the pre-existing CI and `docs/reviews/` changes. No push or pull request is included.

## Evidence and implementation

- [x] Verify the current release executable against the previous source record.
  SHA-256: `52cc99d95bfd7450f5177037d07c8db5741496b321d55bac8c345408213f3584`.
- [x] Sample all current workloads and a separate measurement-change workload
  without active heap profiling. Hit construction spends measurable work mapping
  touch fragment identities back into rows and sorting large records; guide
  validation repeatedly looks up targets whose exact shared ownership is already
  checked. Avoid attributing the setup-only drag press to the timed motion update.
- [x] Have touch compilation emit private input-region rows. Compact regions and
  fragments in one ordered pass instead of materializing and searching full IDs.
  Keep empty mouse regions when touch geometry exists, and preserve each fragment.
- [x] Sort small indices, then permute the sole hit-record arena in place. Keep
  the exact identity comparator, canonical order and unique-record invariant.
- [x] Let guide overlap validation use targets whose exact catalog ownership was
  already proved by the preceding slot validation. Rebind guides from the existing
  sorted canonical target arena instead of constructing a second temporary map.
- [x] Consume only the allocation prefix or protected suffix of already-sorted
  touch adjacency. Keep the existing broad phase, dense fallback and exact cut
  order; add no second adjacency inventory or persistent cache.
- [x] Complete focused/differential and public consumer verification serially,
  independent review, full comparable timing/heap samples, source fingerprints
  and scoped local commits.

## Results

The [complete measurement record](../performance/2026-11-05-hit-inventory-results.md)
preserves both 180-row CSVs, environment, source digests and remaining limits.
At 1,024 panes changed-update p50 falls 12.3–14.8%, and cumulative requested
allocation bytes fall 9.4–10.1%; retained checkpoints are unchanged. The full
workspace passes 2,081 tests, followed by all public consumer, native, Clippy,
documentation, formatting and dependency gates. Three independent read-only
reviews find no evidenced P1/P2. No physical GUI or GPU performance qualification
is claimed. The next performance slice requires fresh attribution; complete
changed updates still exceed 16.7 ms at 1,024 panes.

Raw experimental artifacts live outside the checkout at
`/tmp/dockspace-scale3-01a1a5fb/`. Final reproducible evidence belongs in
`docs/performance/`. All Cargo builds use one job and existing targets; complete
performance runs do not overlap Cargo or CPU sampling started by this session.
