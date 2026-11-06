# Bounded small-tab MRU validation

Date: 2026-11-06. Follow-up implementation after the first frozen checked-Move comparison. The Rust best-practices skill read earlier in this lane was reused. Only `crates/dockspace/src/graph.rs`, `crates/dockspace/src/validation.rs`, and this receipt were written. No Cargo, test, benchmark, C++ compilation, Git write, or subagent was used.

## Change

`graph.rs:15`, `is_exact_tab_mru`, now rejects absent MRU, length mismatch, or a selected item that differs from the MRU head before constructing any set. It uses explicit paired-slice cases for empty, singleton, and two-item stacks:

- Empty slices require no selected item through the shared head check.
- A singleton requires its exact one member in the MRU.
- Two items require distinct visual members and either exact permutation, with the selected-first condition already checked.

All larger slices retain the previous BTreeSet uniqueness/membership algorithm. There is no configurable threshold, alternative long-stack algorithm, new dependency, or retained validation cache. The boolean validity relation is unchanged; only allocation and early rejection placement differ.

`validation.rs:604`, `Validator::validate_tab_mru`, retains the missing-MRU diagnostic. It returns early only when `items.len() <= 2` and the exact predicate succeeds. Such a successful stack would emit no diagnostics in the old MRU method, and this method has no persistent mutation beyond its errors vector. Every invalid small stack and every larger stack continues through the original length, duplicate/foreign/missing-member, and selected-head diagnostics in their original order.

The fast return is confined to MRU validation. `validate_tabs` still checks selection and empty-noncentral status before calling it, and still records every visual item in the global duplicate-owner map afterward. Cycle/shared-node, strict topology, central, presentation, canonicalization, item conservation, and transaction rollback paths were not changed.

The two-label benchmark fixture is the bounded common case this targets: canonical input MRU checking no longer constructs two temporary BTreeSets, and strict MRU validation no longer constructs its two temporary HashSets for exact valid stacks. This is a source-level allocation-removal statement, not a measured improvement or a guarantee about allocator-call counts after optimization.

## Regression coverage

Added `graph::tests::exact_tab_mru_requires_selected_first_unique_membership` (`graph.rs:691`): an explicit small value table covering valid empty/single/two-item permutations, missing MRU, length mismatch, absent/wrong selected, duplicate visual members, duplicate MRU members, foreign members, plus one valid and one invalid larger-stack control. Expected values are stated directly; the test does not implement a set-based oracle or reproduce the production matching algorithm.

Added `validation::tests::strict_validation_checks_small_tab_stacks_completely` (`validation.rs:995`): the real workspace builder/strict validator checks explicit valid and invalid empty/single/two-item cases. Empty valid stacks are central deliberately, so the test does not accidentally redefine empty-noncentral semantics. Invalid duplicate-item/MRU, foreign-item, length and selection combinations must all remain rejected.

The existing `validation_preserves_preorder_and_tab_diagnostic_order` regression (`validation.rs:918`) is untouched. Its malformed first stack takes the full diagnostic path, and its otherwise valid second singleton takes the new fast path while still reporting the cross-stack duplicate item through `validate_tabs`. Its exact eleven-error assertion therefore also guards the surrounding duplicate-owner behavior and invalid-diagnostic ordering.

## Verification and handoff

Successfully ran:

```text
rustfmt --edition 2024 crates/dockspace/src/graph.rs crates/dockspace/src/validation.rs
```

Reviewed the actual diff and predicate call sites. The existing canonical input validator (`canonical.rs:442`) automatically uses the bounded predicate without editing canonical.rs. No runtime test, compilation, or measurement was run by this lane. Root should qualify both new tests, the unchanged exact diagnostic-order test, and existing canonical/transaction coverage in the serialized execution lane before repeating the frozen matched comparison.

There is no known source correctness blocker. The remaining limits are execution qualification and measured efficacy: malformed stacks intentionally still pay for complete diagnostics, larger stacks retain their set work, and other global transaction/layout passes remain. No statement is made that this change alone reaches the 20% four-view investment criterion.
