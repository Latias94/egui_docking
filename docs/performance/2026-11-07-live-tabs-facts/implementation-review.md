# Live Tabs facts implementation review

No source-level blocker found in the exact frozen candidate. It implements the proposed narrow Live Tabs-only deletion: strict reference verification remains first, the original owner/rule queries derive all current facts, and only the temporary recaptured target and its repeated admission are removed. Indexed capture and all edge validation remain on their original paths. The added paired test exercises real transactions and the principal error-priority risks.

This is a source-only review, not qualification or a performance result. I performed no Cargo/build/test/runner/profile/timing/network operation or source modification. I did not repeat disassembly or read another agent's stop opinion.

**Exact package.** Reviewed `/tmp/dockspace-imgui-twofold-20261107-ea4161e/post-main-continuation/live-tabs-facts-source/`. All three `base/` files match immutable `c7508eec3ac44138305797691244ae9c180d7680`. I reconstructed each candidate from the patch in memory and matched every resulting byte. The diff is limited to four hunks: two in `operation.rs`, one visibility change in `workspace.rs`, and one test insertion. Runtime is +18/−9 lines; the test adds 42 lines.

| Artifact | SHA-256 |
| --- | --- |
| `runtime.patch` | `57e65ea61be7f8cdb58681a9fb3d037dc0dc322b6d98e487862a1e933203fe56` |
| `predeclared.json` | `914a969a44584762685d2f5d5f09ffe9f668f26c02e37b1c9d7b951ab13d283c` |
| Base `operation.rs` | `7586ec06b29b77a794133e0c554d4fcca7a6faa16b8c550b86034ad394175862` |
| Candidate `operation.rs` | `985565e79e9246334584e657af0dcc6a130a3ec1eb82cb4fa781b447f6bb3f35` |
| Base `workspace.rs` | `6cbcca776bfca60b1e767dc909121f06ae751110ea1f9fd265ea398da644aa36` |
| Candidate `workspace.rs` | `df965e6fb2e78c0a9316bcd347612ad35da9a899151233567f0b846c2866edb2` |
| Base `operation/tests/checked_move.rs` | `410baabecc85197a32867237620ec05fdb1ecd96ebb6a510cae61729d647a602` |
| Candidate test file | `eee4540669f907253478e00a138aca9f1ee6dd55828ebed61f6d85efc37efe25` |

Paths and line numbers below refer to `candidate/crates/dockspace/src/`. The saved design input, `source-review.md`, matches the earlier report SHA-256 `4eae2d9c001cc51bce8020385b00f3f8ec19639c85a05e93e0c157582631bc21`.

**Strict identity and local equivalence.** `validate_tab_target` still invokes `references.verify_reference` before its matcher (`operation.rs:2944–2951`). That verification and `checked_root_reference` are unchanged: root existence, complete-generational node existence, actual root membership and exact issued Arc identity remain required. Stale errors still own the original expected/actual references. No value-equality or revision shortcut replaces issuance.

The Live matcher (`2706–2719`) derives `(surface, central)` with the original `target_owner_facts`, then `rule` with the original pane-local query and the **derived** central flag. Both fallible calls execute before the boolean comparison, even when surface already differs. The immutable workspace borrow prevents mutation between reference verification and derivation.

The old Live capture constructed root/tabs directly from the same input and cloned the issuance just verified. Those fields therefore add no independent admission after successful verification in this immutable borrow. Surface, central and rule are the remaining state-derived fields, and the matcher compares all three. It does not trust captured central/rule values to derive current facts.

Only `target_owner_facts` visibility becomes `pub(crate)` (`workspace.rs:719`); its implementation and every public capture method are unchanged. There is no new target representation, stored validity flag, proof token, provider, generic abstraction or cache.

**Error and authority boundaries.** The original facts-mismatch invariant remains `"rederive frozen tabs target facts"`, followed by the unchanged node match and optional `TabIndexOutOfBounds` check (`operation.rs:2951–2976`). Thus owner/pane derivation errors still precede mismatch, mismatch precedes gap bounds, and reference errors precede all three. The existing missing-presentation invariant and empty-central Root rule are retained by the same queries.

The Indexed arm (`2721–2725`) still calls checked `index.capture_tab_target` and compares the complete result. Version, indexed membership/issuance and live expected-reference checks are unchanged; moving equality into the helper does not weaken them. Inner and outer capture wrappers and `validate_edge_target` are unchanged, including scope priority, actual root-node derivation and fraction comparison.

Candidate/staged mutation, policy execution, MRU, root issuance, publication, rollback and owned outputs are outside this diff. The matcher has one caller, immediately after verification; no successful match can be reused across another command or edit.

**Meaningful test coverage.** `tab_target_transaction_preserves_derivation_mismatch_and_gap_error_order` (`checked_move.rs:223–263`) constructs two cases using the existing valid three-root fixture, a current target reference, forged surface and an oversized gap:

- Repointing the target to the actual Split in the same root keeps membership/reference valid. Pane-rule derivation must return `NodeIsNotTabs` before the forged surface mismatch or gap error.
- Keeping the actual Tabs node allows derivation to finish; the forged surface must return the exact facts-mismatch invariant before gap bounds.

Both use `WorkspaceTransaction::apply`, assert the full command-error wrapper/index/variant, and compare the workspace to its pre-transaction snapshot. This is a real failure/publication witness, not a test that mirrors a private helper or assumes a capacity/clone count.

Existing forged surface/central/rule cases, staged stale-target and foreign-role tests remain unchanged. The new test does not itself exercise a late second-command rollback; existing transaction tests continue to cover that separate contract. No new framework or additional semantic test is necessary for this source delta.

**Qualification limit.** The source removes the second Live Tabs membership/issuance replay and temporary owned reference, but does not prove a machine-code or complete-operation benefit. Existing work counters should remain equal because they do not count RootReference atomics. Full ordinary/work/vector qualification and the predeclared affected center/cross comparison remain Root's responsibility. Any measured regression must be retained in the evidence; this source approval does not override the experiment's retention rules.
