# Rejected experiment: reuse solver aggregates

Decision: discard this experimental production change and restore the exact
qualified leaf-reader source at `5ea7fbb`. The arithmetic was correct, but the
complete operation did not show a consistent material gain. The experiment's
patch, raw results and checks remain available rather than being counted as an
optimization retained in the library.

The candidate returned the total weight already computed by validation to the
noncentral desired-size calculation. It also retained the first assigned-size
sum when neither grow nor shrink changed the sizes. Central sibling sums,
post-mutation sums, accumulation order, maximum-overflow priority, numerical
tolerances and complete rich output stayed unchanged. The frozen independent
solver still performed both original sums.

The one-file patch passed 77 focused and 2,225 workspace tests, formatting and
correctness/suspicious Clippy with normal counters. Independent numerical/error
review found no blocker. The frozen production binary used the original work
matrix, fresh inputs and complete owned export, with 48 samples/eight warmups
and two serial ABBA/BAAB groups against both the qualified leaf-reader binary
and the pinned ImGui kernel. Every complete vector and sampled digest agreed.

Against the leaf-reader baseline, 14 of 42 group/cell medians regress; only
eight of 21 cells improve in both groups. At 1,024 panes the median changes span
a 1.08% regression to a 3.10% improvement, with several group directions
disagreeing. Across the whole matrix the range is a 6.84% regression to a 9.99%
improvement. Eighteen p95 cells and twenty max cells regress; the largest
regressions are 131.04% and 1,654.42% respectively. The observations do not
justify the additional argument/branch structure as a performance change.

No applicable cell reaches twice ImGui. This small arithmetic experiment is
not a failed alternative architecture or proof of a lower bound. Candidate
copying, complete-output access, cold effect ownership and layout allocation
remain distinct practical directions and are still being tested.

The [archive](2026-11-07-solver-aggregate/evidence.tar.xz) contains 408 input
files, 16,128 raw rows and 336 complete vectors, exact source/binary receipts,
checks, the unapplied-after-rejection patch and independent review. All members
were read back byte for byte. [Comparisons](2026-11-07-solver-aggregate/comparisons.csv)
retain every tail and negative cell. [decision.json](2026-11-07-solver-aggregate/decision.json)
records the exact restored source hash;
[SHA256SUMS](2026-11-07-solver-aggregate/SHA256SUMS) identifies the artifacts.
