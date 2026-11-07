# Independent source-nonempty retention challenge

**Recommendation: reject this measured candidate under the existing resize guard.** The targeted small-move improvement is credible and the numerical acceptance counts pass. However, the evidence does not justify certifying “no repeatable material resize regression.” Keeping it would require acknowledging a product tradeoff rather than claiming that all predeclared conditions were met. No fourth campaign is requested.

I independently recomputed the comparisons from `docs/performance/2026-11-07-source-nonempty/`, read both measurements JSON files, and applied the original `source-nonempty-source/predeclared.json`. Ordinary qualification and work equality are accepted as supplied; they were not rerun or independently re-audited here.

| Complete p50 result | Initial Rust comparison | Additional Rust confirmation |
| --- | --- | --- |
| Moves: improve / regress / tie | 21 / 9 / 0 | 21 / 8 / 1 |
| Small moves | 10 / 0 / 0 | 9 / 1 / 0 |
| Resize | 1 / 10 / 1 | 5 / 6 / 1 |
| All cells | 22 / 19 / 1 | 26 / 14 / 2 |

Both runs satisfy at least 20/30 improved move comparisons and 6/10 improved small moves. The formal ImGui campaign has 30/42 at most 2×, with all medium/large cells passing. Confirmation does not replace the first campaign.

## Why the resize guard remains a retention obstacle

The guard says to reject repeatable material regressions; it does not require every negative cell to be rejected, and it gives no fixed percentage. My judgment rests on repeated medium/large evidence, not an invented cutoff or the single largest initial small-cell loss:

| Resize workload | Initial groups: p50 reduction | Confirmation groups: p50 reduction |
| --- | --- | --- |
| 128 panes / 1 view | −3.26%, −4.86% | −2.00%, +1.95% |
| 1024 panes / 1 view | −3.20%, −2.99% | −2.18%, +0.30% |
| 1024 panes / 4 views | −3.81%, +1.05% | −3.70%, +0.80% |

For 1024/1, three comparisons lose 1.54–2.29 microseconds and the fourth gains 208 ns. Its p95 reductions are −12.40%, −3.01%, −3.81%, and −0.82%: all four are adverse. The 128/1 p95 is also adverse in all four comparisons. Together with the first campaign's broad resize regressions, this is enough repeat evidence for a conservative rejection of an optimization intended to leave resize unaffected. It is not a claim that the patch's source-level control flow uniquely explains those latencies.

The strongest counterargument must remain visible: confirmation group 1 regresses every resize median, while group 2 improves five and ties one. This demonstrates substantial group sensitivity and argues against declaring a universal resize penalty. It does not erase the first campaign or establish that the repeated losses are immaterial. Likewise, the initial 16/1 loss of 14.22% was not repeated at that magnitude; I do not use it as the decisive veto. The 1 ns confirmation loss there is not the material concern.

Targeted benefits are also mixed by scale: initial move medians improve 10/10 small and 10/10 medium comparisons, but only 1/10 large; confirmation improves 9/10, 7/10 and 5/10 respectively, with one medium tie. This is a small-workload winner, not a general move winner. That distinction was allowed by the predeclared affected/small counts, so it is context rather than an additional rejection rule.

Initial complete p95 has 22 regressions; confirmation has 14. Maxima contain large favorable and adverse outliers in both runs. Preserve them, but do not turn the worst maximum alone into a stable causal argument. The repeated resize median/p95 pattern is the stronger retention concern.

The shortcut remains source-correct and removes genuine redundant work. Neither fact guarantees a retained complete-operation improvement. Record rejection of this binary, retain its complete evidence, and continue from the actual retained control. This decision neither declares the overall 2× goal achieved nor rules out a future independently motivated implementation.

Bindings:

- Initial CSV: `58eb01e94a459a840480dfc7cde50ce1494b171f70bf98215b27555a5df415a5`
- Confirmation CSV: `68047b8beb5c091c15b7686f0a12aeeeea85b411ab53fa77f4b7199338b19ecf`
- Initial measurements: `9c36d3711e124c32fb873b5282ec2f3ece74bd62b483f0e0febdb98545005aba`
- Confirmation measurements: `b6acdb141fde29e6c849613e88eef4c5c7387ef9183e2519be0077538450b1aa`
- Measured candidate binary: `0ac7566065a4a7b23b88b2c7f0c690122d0ae6cd4e7227693f85f29b73e48317`

Only static evidence reading and CSV arithmetic were performed. No other independent review was consulted; no build, test, runner, profile, timing, network request, or repository modification was performed.
