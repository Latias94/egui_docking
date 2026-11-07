# Target recapture: current-control source and executable review

Finding: the temporary target recapture is real work in the current control, not source duplication that the compiler has already removed. Live validation first verifies a borrowed root reference, then capture repeats root/node/membership/issuance checks and creates another owned RootReference solely for comparison and destruction. A small **Live Tabs-only** experiment is reasonable: keep the initial verification and original scalar-fact derivation, compare those facts without constructing another owned target, and leave indexed and edge paths unchanged for the first screen. Quantitative benefit remains unknown.

This is not a performance result or a recommendation to relax authority. The source-matched sample observes the call path, while static disassembly confirms the surviving checks and atomics. There is no allocator saving: cloning a RootReference retains its existing Arc allocation.

**Exact basis.** Source is immutable `c7508eec3ac44138305797691244ae9c180d7680`, retained runtime `0e441c8`; the sampled executable is `/tmp/dockspace-imgui-twofold-20261107-ea4161e/post-main-continuation/current-main-profile/diagnostic_docking_cost`, SHA-256 `35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590`. It has the previously audited one-line probe-cap change, not a candidate runtime. Live source changes for other candidates are excluded.

| Immutable source, relative to `crates/dockspace/src/` | SHA-256 |
| --- | --- |
| `operation.rs` | `7586ec06b29b77a794133e0c554d4fcca7a6faa16b8c550b86034ad394175862` |
| `workspace.rs` | `6cbcca776bfca60b1e767dc909121f06ae751110ea1f9fd265ea398da644aa36` |
| `command.rs` | `247069a521a079b0a2d64b232301b751ccfb4cedc42ab2493685f3865afa0594` |
| `graph/authority.rs` | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |

I read only this task's sources and saved current samples, and used `xcrun llvm-objdump` for static executable inspection. No Cargo, tests, workload, profiling, timing, network operation or source change was performed. I did not read either stop review.

**What currently repeats.** `validate_tab_target` (`operation.rs:2928–2967`) calls `ReferenceAuthority::verify_reference`, captures another TabTarget, compares the entire value, then looks up the Tabs node again for optional insertion-index validation. Live capture (`workspace.rs:626–641`) calls `capture_reference`, `target_owner_facts` and `pane_local_target_rule`.

`capture_reference` calls the same `checked_root_reference` used by verification, then clones it (`workspace.rs:743–789`). The checks are missing root, missing full-generational node, actual root placement, and issued reference availability. Successful verification compares borrowed references; only the stale-error path clones its expected/actual error payloads. The later success-path capture creates an unnecessary temporary owner, without changing issuance.

Inner edge validation has the same shape (`operation.rs:2895–2925`, `workspace.rs:651–672`). Outer capture additionally derives the actual node from the root record, a distinction that must survive any later redesign. These checks are root-local map/placement operations, not the former whole-root fingerprint reconstruction.

The indexed variant contains more repeated admission work, but carries a different contract. `WorkspaceIndex::verify_reference` first calls index `capture_reference` (version, index root, live root/node/membership, index issuance), then verifies the supplied expected reference. Indexed target recapture calls admission again and clones the indexed reference. Deleting those checks by substituting live facts would discard index/version authority. It is unnecessary scope for a small ordinary-command experiment.

**Executable evidence.** Preferred Mach-O addresses, without process slide:

| Function | Surviving operation |
| --- | --- |
| `validate_tab_target::h6a9994a3c6cfb6c8`, start `0x1009cb8cc` | Calls reference verification at `0x1009cb930`; Live capture at `0x1009cb9e4`, return offset **284**. |
| Live `capture_tab_target::hf7ec143a57b64180`, start `0x100bb8620` | Repeats `node_root` at `0x100bb86ec`; success-path Arc retain is `ldadd +1` at `0x100bb87a8`. |
| `validate_tab_target` comparison/cleanup | Compares actual reference pointer at `0x1009cba3c`; shared temporary-owner decrement is `ldaddl -1` at `0x1009cbc2c`, reached after successful comparison/index checking as well as mismatch. |
| `validate_edge_target::h8b53f51e53480090`, start `0x1009cc0ec` | Calls verification at `0x1009cc158`; Live inner capture at `0x1009cc314`, return offset **556**. |
| Live inner capture, start `0x100bb99a4` | Repeats `node_root` at `0x100bb9a80`; Arc retain at `0x100bb9b44`. |
| Edge successful cleanup | Arc decrement at `0x1009cc3bc`; the outer capture call at `0x1009cc290` also remains. |

Current center sample lines 1362–1363 have timed `prepare → apply_command → check_move → validate_tab_target +284 → capture_tab_target` ancestry. Current edge sample lines 1238–1242 similarly reach inner recapture at `+556`, including a sampled PC immediately after the actual Arc increment. There is no fixture ancestor, and the probe caller is the previously identified timed `+1828` callsite.

These grouped observations establish applicability, not an exclusive share, atomic latency, allocation count or a complete-operation lower bound. The check/capture functions are not optimized away despite having related source logic.

**One bounded seam to test.** Limit the first candidate to the private TabTarget matcher in `ReferenceAuthority`, used by `validate_tab_target`. Keep `references.verify_reference(...)` exactly where it is. Replace its subsequent owned-capture equality with:

1. Live: derive current `(surface, central)` through the existing `Workspace::target_owner_facts`, then derive `rule` through the existing `pane_local_target_rule`, and compare these three Copy fields to the captured target.
2. Indexed: retain the existing checked indexed capture and full equality.
3. Preserve the original mismatch error stage and subsequent node-kind/insertion-index match unchanged.

The only necessary workspace interface change is making the existing private `target_owner_facts` query accessible within the crate; its implementation and the public capture functions can remain unchanged. The matcher can replace the current private capture wrapper, rather than creating a second target representation, provider, proof token, generic verifier or persistent “already checked” flag. An immutable `&Workspace` borrow covers both verification and derivation; nothing can edit the graph between them.

For Live Tabs, root and tabs in the recaptured value are precisely the input root/tabs, and its reference is the current issuance already compared to the expected reference. The remaining state-derived fields are exactly surface, central and rule. Computing all three with the original queries therefore retains the substantive frozen-facts check while deleting the second membership/issuance replay, temporary owned reference and full target reconstruction.

This is a two-runtime-file change, plausibly within about forty added lines. It affects ordinary center/TabGap and cross-view merges, not edge/resize or indexed eligibility. That is enough to give the mechanism a fair bounded screen; it need not absorb every related check to be worthwhile.

**Authority and error constraints.**

- Keep the first reference verification and its full-generational membership/strict Arc identity. Independent builds, foreign roots, retired/reused node keys, edit/selection roundtrips and earlier staged edits must continue to reject; value equality or same version is not sufficient.
- Always compute current central from the root record, not `target.is_central()`. Derive current rule from actual selected item/empty-central state, not `target.rule()`. A current reference does not authorize forged policy fields; `checked_move.rs:184–220` proves that distinction.
- Derive both owner facts and the pane rule **before** deciding a field mismatch. A current target forged to point to a Split can also have a forged surface: old capture returns `NodeIsNotTabs`, not the later invariant mismatch. An unselected noncentral Tabs similarly keeps `TargetTabsUnselected`.
- Preserve reference errors before all derivation/mismatch/index errors. Preserve frozen-facts mismatch before `TabIndexOutOfBounds`, and leave payload validation before target validation and semantic/policy checks afterwards (`operation.rs:1201–1218`).
- Preserve the empty central leaf's Root rule and the exact missing-presentation invariant. Do not shortcut fact checking just because production constructors normally produce valid targets.
- Leave stale error ownership intact: cloning RootReferences into a real `StaleNode` error is required output, distinct from the success-path temporary owner being removed.

**Why not fold in edges immediately.** Inner edges could use a similar live comparison, but full equality also compares the echoed fraction. In this executable `fcmp` at `0x1009cc37c` is real. Public `DockFraction::new` guarantees finite `(0,1)` values; if preserving arbitrary in-module invalid fraction behavior is required, a scalar-only rewrite must account for NaN non-reflexivity instead of silently dropping it. No new fraction validation should change error order merely to simplify this experiment.

Outer edges are more delicate: verifying the supplied target node proves it is a current member, not that it is the root node. The old capture derives `RootRecord.node`, validates that node and compares it. Reusing the supplied node would authorize a forged outer boundary or alter malformed-root errors. Indexed variants additionally require the original version/index issuance checks. Leaving these paths unchanged keeps the proposed screen small and its proof local.

**Qualification and decision.** Reuse existing forged-policy-facts tests, stale-selection/earlier-command tests, foreign-role tests, empty-central capture tests, indexed capture equivalence and index-version/rehome tests. A small priority witness combining a current wrong-kind target with a forged policy field, and forged valid Tabs facts with an out-of-bounds gap, would protect the principal new ordering risk; no new random framework is needed.

The initial executable evidence justifies considering this candidate after the currently running variant is decided. It does not justify optimistic retention. One separate early screen against the actual frozen Rust control should examine affected complete center/cross cells, all negatives and unchanged work/vectors, with rules declared before measurement. If it needs a larger target-authority redesign or if the screen fails, defer it. Neither the failed Tabs capacity experiment nor a desire to stop makes this distinct ownership deletion automatically good or automatically exhausted.
