# Rejected experiment: inline projection continuation stack

Reject the exact SmallVec16 continuation and all accompanying dependency
edges. Restore all eight source/configuration paths byte for byte to `eb94cb2`.
The candidate changes only the production continuation container, retaining
its original frame enum, push/pop order and normal dynamic spill. The existing
locked SmallVec 1.16.4 gains a direct core dependency through the workspace
manifests and five applicable build locks; no package is upgraded. Historical
evidence locks remain unchanged.

The [source review](2026-11-07-smallvec-stack/combined-source-review.md)
finds no source blocker. Deep stacks retain dynamic growth, and complete
validation, numerical arithmetic, fresh inputs and owned outputs remain.
All twelve ordinary counter-enabled gates pass, including the existing deep,
raw-DAG, late-error, old-output and independent-candidate witnesses. The full
21-row, 20-column work CSV equals the packed control byte for byte: six
successful root bodies and 2,926 rows. It supplies logical-work evidence,
not allocator counts or time shares.

Frozen candidate SHA256:
`f507e879e7350dff66ee8e52dd226b19a63d7854cda68f67fcafb3977bda0b5a`.
Actual control is `eb94cb2`, binary
`21888a3b2905da0edce33f68d4a7e3dbf68ba0e45d65f1d72df5d4d03f110435`.
Both control and pinned ImGui comparisons use serial ABBA/BAAB groups,
48 samples/eight warmups per process/cell and 96 observations per condition/
group/cell. All complete vectors and digests agree. The timer retains request
capture, checked mutation, fresh all-root leaf inputs/layout and fresh complete
owned export.

All 42 complete p50 group/cells regress against the retained control, by
2.52–33.01%. All fourteen large group/cells regress by 9.77–22.17%. P95 has
39 regressions, range -141.77% to +12.72%; max has 23 regressions, range
-495.23% to +84.06%. The [full table](2026-11-07-smallvec-stack/comparisons.csv)
preserves every aggregate comparison; the archive preserves each observation.
An inline stack owner therefore fails to earn its added dependency, storage
footprint and branch/code-generation tradeoff in this implementation.

| Total panes | Complete p50 candidate / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.33–4.00x | 0/14 |
| 128 | 1.41–1.96x | 14/14 |
| 1024 | 1.03–1.40x | 14/14 |

The rejected variant happens to retain 28/42 at the threshold, despite every
complete median regressing against its actual control. All 42 p50, 41 p95 and
34 max cells trail ImGui. This does not improve the retained result or satisfy
the goal. Every small cell remains above 2x.

Negative costs include sixteen inline enum slots, inline/spilled branches,
possible relocation and spill copies, and altered compiler decisions. The
current binary fixtures fit the source-derived capacity bound; general deep
workspaces still spill. Fewer source-level heap owners do not identify an
exclusive cause for either the earlier samples or this regression. No allocator
time share, allocation-byte saving, exact stack size or architecture-wide lower
bound is established.

The [archive](2026-11-07-smallvec-stack/evidence.tar.xz) contains 449 inputs
plus its manifest, 16,128 raw observations, 336 complete vectors, exact
control/candidate source/configuration pairs, ordinary qualification/work,
and the original reviewed source package. Every member round-trips against
the original bytes. The [decision](2026-11-07-smallvec-stack/decision.json)
binds the frozen delta and verifies all eight restored identities. The
separate native failure diagnostic remains retained.

Continue the original standard-Vec continuation with only its redundant
control-flow roundtrips removed, then the other independently reviewed
numeric/storage alternatives. This failed container substitution is not a
stopping decision.

The [independent evidence review](2026-11-07-smallvec-stack/evidence-review.md)
reproduces all recorded statistics, vector equivalence and immutable eight-path
restoration. Its only correction is the displayed worst large-cell regression,
rounded here to 22.17%; exact comparison data and the rejection are unchanged.
