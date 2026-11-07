# Equal-binary final decision

**Reject and retain the actual control. No fourth campaign is needed.** The additive confirmation does not sustain the initial acceptance result, and the original tail concern is only partly resolved. Keeping the first campaign while discarding the requested confirmation would be selective retention.

Independent static recomputation:

| Result | Initial campaign | Additive Rust confirmation |
| --- | --- | --- |
| Complete p50 improve / regress / tie | 34 / 8 / 0 | 20 / 20 / 2 |
| Small layout p50 improvements | 13 / 14 | 6 / 14 |

The original targets were at least 28/42 complete p50 improvements and 8/14 small layout improvements. The first campaign passes both; confirmation falls below both. This does not retroactively make the first result false, nor introduce a new percentage threshold. It shows that the apparent retention benefit was not reproduced by the specifically requested additional comparison.

Tail evidence is mixed rather than uniformly adverse. Confirmation 1024/4 center max still regresses **57.75% and 36.41%**, while its p95 changes by **+4.26% and −8.36%**. Cross-view p95 changes by **−2.53% and +3.00%**. Small single-view edge p95 now rises from 3958 to 4125 ns in group 1 but improves from 3667 to 3583 ns in group 2. These do not establish a universal tail penalty or an exclusive source-level cause. They also do not rescue the unconfirmed broad median/layout benefit. The decision need not rest on maxima alone.

The rejected candidate's 31/42 ImGui inventory remains an honest campaign observation. It cannot be used to override the actual-control evidence or to claim improvement over the retained historical 30/42 count.

The exact-equal-weight specialization remains mathematically sound and passed qualification. This result rejects its measured implementation as a performance winner. It proves neither a numerical lower bound nor the impossibility of another same-contract binary algorithm. No runtime or added test from this treatment should remain.

Bindings:

- Confirmation comparisons: `45498b0c49181bd8eb619436a33a2617e381650ffc0311eedf42bead265bf27a`
- Confirmation measurements: `059ad92438487d4eadb768edf2d0f7da2bc5bf1b4f18159d32e6a06e0d7346ed`
- Confirmation phase summary: `13d22af95d5723c7b0e82ffa324e47d34f9854a13b070e281d449a7865c5857d`
- Original review remains unchanged: `feef5d68c626b189245001406dd678802e8ba1dca211bb426ce4599925bf10af`

The live `layout.rs` and `layout/topology_tests.rs` hashes match the restored control (`01f21a…5dcd` and `676718…1ad2`). The decision receipt records complete 324-source restoration; this short review did not re-audit that inventory or archive.

Only static evidence reading, CSV arithmetic and hashing were performed. No other review, build, test, timing, runner, profile, network request or repository edit was used.
