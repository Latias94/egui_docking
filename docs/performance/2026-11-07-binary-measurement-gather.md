# Binary local measurement records with unchanged slice composition

Retain the two bounded measurement-gather changes. Exactly two child rows
produce two local SubtreeMeasurement records, then call the original slice
composer with the same ordered values. All other arities retain the reusable
Vec and original clear/extend path. Apply the same choice only at the existing
delayed-maximum gather when that pass actually runs. No composer, numerical
executor, validation or fallible-event order changes.

A fully binary pass no longer populates the temporary child-values heap owner;
the previous Vec was reused, so this is not an allocation removed per split.
Both records remain materialized. Mixed/nary paths retain their allocations,
while branch/local footprint and compiler decisions can regress. Fresh leaf
answers, all three rich result buffers, min/max/actual/gap outcomes, old-output
ownership and complete fresh export remain. Retained exact-total reuse is
preserved; no rejected axis, SmallVec, fused or persistent treatment returns.

All twelve ordinary counter-enabled gates pass, including the existing
independent numeric/full-layout oracles, nary/raw/central/delayed/deep cases,
staged candidate/MRU and old-output lifetime. The complete 21-by-20 work CSV
equals actual control byte for byte. Both serial ABBA/BAAB campaigns preserve
all vectors/digests with 48 samples/eight warmups per process/cell and 96
observations per condition/group/cell. Original capture, checked mutation,
fresh all-root layout and complete fresh owned export remain timed. The
first formatting check and the equivalent rustfmt fallback closure adjustment
are separately preserved; no semantic failure is hidden.

Candidate SHA256: `6d255458eac25cd9dcf629b88cfca6959c78db7ed2bb7ccf08fe2cba6332dcfd`.
Actual control is 37e388f, frozen exact-total binary
`ccbc1be2fe9d0c14b63e8fbed845fda6332148786485c539b9166d6efdcca5f3`.
Complete medians improve 35 cells, regress 5 and tie 2, range -4.13% to +16.67%.
All fourteen large cells improve 1.23–5.59%. The five negative medians are:

- Group 1, 16 panes/4 views/cross_view_move: -1.79%.
- Group 1, 128 panes/1 views/resize: -4.13%.
- Group 1, 128 panes/1 views/center_move: -1.58%.
- Group 1, 128 panes/4 views/cross_view_move: -1.89%.
- Group 2, 16 panes/1 views/center_move: -3.83%.

P95 has 26 improvements and 16 regressions, range -42.47% to +67.94%; max has 20
improvements and 22 regressions, range -936.68% to +97.35%. All negative and
favorable tails remain in the [complete table](2026-11-07-binary-measurement-gather/comparisons.csv).
The change earns a bounded complete gain, not a general allocator or language
conclusion. No source-owner count is converted into a timing fraction.

| Total panes | Complete p50 candidate / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 1.7655–3.4656x | 2/14 |
| 128 | 1.2644–1.5929x | 14/14 |
| 1024 | 0.8483–1.1343x | 14/14 |

The result improves from 28/42 to 30/42. Small single-view resize reaches 2.0x
and 1.7655x; the remaining twelve small conditions still miss. Against ImGui,
39 p50, 34 p95 and 28 max cells trail. There is no all-cell completion claim.

The [archive](2026-11-07-binary-measurement-gather/evidence.tar.xz) preserves
451 inputs plus manifest, 16,128 raw observations, 336 complete vectors, all gates,
source/review/formatting pairs and separate author-eb94cb2 and actual 37e388f
bindings. The [decision](2026-11-07-binary-measurement-gather/decision.json)
retains only this measured two-path delta. Obtain the bounded source-matched
small resize/edge call-tree observation on this new frozen binary, then ask
independent review whether a concrete further optimization is justified.

An [independent evidence review](2026-11-07-binary-measurement-gather/independent-evidence-review.md)
recomputed all statistics and checked all 451 inputs, twelve gates, work rows,
16128 raw observations and 336 vectors. It found no blocking source or evidence
binding issue. Its SHA-256 is
`94ca9d5dc0246f7648e9ccfce3e3d4bff5e2bf6695afe6045ffa6d38b804f2b9`.
