# Equal-binary retention challenge

The numerical acceptance gates pass. I recommend **one unchanged complete Rust confirmation before retaining**, specifically to resolve repeated adverse large/four-view tails under the predeclared tail guard. This is not a request to repeat qualification, the ImGui comparison, archive auditing, or a capacity/parameter sweep.

I recomputed both comparison summaries and the selected layout count from the existing CSVs. Ordinary correctness/work/full-output qualification is accepted as supplied and was not rerun.

| Predeclared requirement | Observed | Decision |
| --- | --- | --- |
| At least 28/42 complete p50 improvements | 34 improve, 8 regress, no ties | Pass |
| At least 8/14 small layout p50 improvements | 13 improve, 1 regress | Pass |
| At least 30/42 at most 2× ImGui; all medium/large pass | 31/42; all medium/large pass | Pass |
| Disclose tails; repeated material negative tradeoffs block retention | p95: 20 improve / 22 regress; max: 24 / 18 | Specific concern below |

The direct control-relative complete p50 reduction range is −4.06% to +10.00%. The eight negative medians remain part of this decision. Seven do not repeat in both groups. The sole repeated negative median is 1024/1 edge: −0.80% and −0.21%, while its p95 changes direction and both maxima improve. Those median negatives alone do not establish a material repeated penalty requiring confirmation.

The stronger concern is concentrated in the large four-view move tails:

| Workload | Group 1 / 2 p95 reduction | Group 1 / 2 max reduction |
| --- | --- | --- |
| 1024 / 4 / center | −16.51% / −8.46% | −154.99% / −96.28% |
| 1024 / 4 / cross-view | −8.88% / −4.53% | −130.80% / −48.34% |

For center, complete p95 rises from 88,834 to 103,500 ns and from 87,625 to 95,042 ns. This repeats in two groups while its complete median improves only 1.25%/0.58%. Cross-view tails show the same adverse direction in both groups. These observations are sufficiently material to require resolving the original tail guard before declaring an unconditional winner; no new percentage threshold is introduced.

Other tails do not independently establish a stable veto. Small single-view edge p95 regresses 9.89%/8.15%, but its layout p95 is essentially unchanged and mutation p95 is adverse. The largest maximum regression occurs only in one group and should not decide retention alone. Phase percentiles do not add up to a complete-operation percentile or prove an exclusive cause.

One complete Rust confirmation is therefore targeted evidence, not routine expansion. Preserve the original campaign unchanged and compare the same frozen candidate/control over the same matrix. If the large/four-view tail penalty repeats materially, reject under the existing guard despite passing median counts. If it does not repeat, a qualified mixed-result retention can be honest, provided both campaigns and all adverse cells remain published. If no further comparison is undertaken, I would leave retention unapproved rather than silently treating the tail guard as satisfied.

The 31/42 ImGui count satisfies this campaign's floor. Comparing it with the historical 30/42 is not a code-gain estimate: that is a separate campaign. Likewise, these results do not establish all-cell completion, uniform improvement, or an allocator/ABI explanation.

Bindings:

- `comparisons.csv`: `7d7fe0e5c070d3d0230f09f80f733c6af53f3688c11d165657b9f59f4b8cdd90`
- `measurements.json`: `d746c68600d72865d57a2ea97bb0c444f239626f922258519f373f12aca5963b`
- Original `predeclared.json`: `5f6bdfeb403e32e6689fa3379682c753c6e02ff8f32238188de0f41eeaf6ea34`
- Candidate binary: `fc17c426ae6c25d747a2231807d187866c30bedde35cdb32c9abd933978998a6`

Only static evidence reading and CSV arithmetic were performed. No other independent report was read; no build, test, profile, runner, measurement, network request, or repository modification was performed.
