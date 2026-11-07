# Post-main result: short document review

November 7, 2026. Read-only review of the current result document and the additions to twofold-result.md and the plan. No source/archive re-audit, network request, Cargo, test, runner, profile, or timing was performed. Only this report was written.

**No numerical, qualification-scope, or CI-obligation blocker was found. Two wording corrections are recommended before publication.**

1. In post-main-result.md:26, replace “The numerical target remains 30/42” with “The retained result remains 30/42; the numerical target is still unmet.” The target must not be redefined as the current inventory.
2. At :109–111, avoid implying that replacing transient split-directory writes requires a “complete structural redesign.” It is an unmeasured local scalar/counting cleanup; the identity sort and final directory remain. Suggested wording: “Temporary split-directory writes remain an unmeasured local cleanup; they do not remove the required identity sort, and currently lack sufficient complete-metric evidence to justify another experiment.” Larger incremental structural designs have separate costs.

The checked content otherwise agrees with the saved records:

- The table independently matches the retained ImGui summary: 16 panes 1.8111–3.1244x and 2/14; 128 panes 1.2285–1.4969x and 14/14; 1024 panes 0.8292–1.1445x and 14/14. Total is 30/42, with twelve small misses.
- Both products' formal topology is binary. Sixteen panes is a total leaf count, not sixteen children. Neither this statement nor the comparison implies a debug-versus-release explanation; both sides use their frozen formal benchmark executables.
- All seven candidates use the actual retained Rust control. The two additive Rust confirmations remain separate from their initial campaigns; favorable candidate/ImGui observations do not replace the retained result.
- The first three candidates completed twelve ordinary gates. The last four completed only the declared three ordinary screening gates, plus work/full-vector checks; no twelve-gate qualification is claimed for them.
- No new runtime winner is retained. The control identity/runtime/hash and exact restoration statements match the reviewed checkpoint. No optimality, universal regression, exclusive compiler cause, or lower-bound claim is added.
- The merged-main six-job receipt is limited to c7508ee. The result, plan addition, and local summary retain the obligation to publish and verify the exact later head's six jobs. This review does not independently verify remote CI.
- The twofold-result and plan additions accurately describe this checkpoint and preserve older stopping reviews as historical assessments.

Root subsequently reported two new concrete opportunities and will screen them separately. Therefore the current seven-candidate section is an **intermediate checkpoint**, not final stopping authorization. Keep both prior stopping challenges as intermediate records, extend the treatment list when new decisions exist, and refresh the final narrative/summary rather than publishing the seven-item snapshot as exhaustive. This document check supplies no opinion on the two new designs.

Reviewed snapshot hashes:

| Document | SHA-256 |
|---|---|
| docs/performance/2026-11-07-post-main-result.md | `4aed96d587c17dbefa0076039645cae7c5470f4fe0d975e296500bb31c4fbe64` |
| docs/performance/2026-11-07-twofold-result.md | `05a8752b4e024c35720f82a86f0104201e0fd89840bbdb30b43ab9610a9179f4` |
| docs/plans/2026-11-07-001-docking-twofold.md | `63f16c18a405d676ecac1df73dde86d14d2b57e6769ef293998555a960ba141f` |

These hashes identify the reviewed drafts before the suggested wording and subsequent experiment updates.
