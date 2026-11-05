# Product cost attribution and ownership changes

Date: 2026-11-05. Base: `868e635`. Status: qualified.

Qualified implementation commits:

- Core junction queries: `80eda8841283e9473b04de3b0dfd9b222a5e61ab`.
- Exact paint auditing and ownership: `32a04bb8b573c63ee2a7acd0f62bdfc36504fb19`.
- Untimed profiling selector: `2f240682a864a24c8f8fcee1b2a398ff8ac7c3bd`.

## What explains the product gap

The main sampled cost is the adapter's repeated paint-output auditing. Idle
does not repeatedly solve the entire layout: exact configuration, measurements,
coordinate and interaction requirements allow the core to share its existing
compiled plan. A changed surface size does solve and compile a new projection,
but isolated resize sampling still attributes far more work to auditing than
to the layout solver itself.

Ownership matters in two ways. Every pane used to create and destroy snapshots
of the growing graphics list, and derived text equality repeatedly traversed
the same immutable galley's layout job, glyphs and meshes. Sharing an Arc does
not make its PartialEq compare only identity: Galley is PartialEq rather than
Eq, and the pinned Rust Arc implementation retains value comparison here.

The final implementation retains all audits, borrows live lists at completion,
reuses exact immutable-galley comparisons and recycles verified pure snapshot
storage within one paint call. At 1,024 panes its p50 falls by 51.55–59.04%
relative to a fresh run of the previous qualified executable. This is a product
observation, not an inherent Rust or layout-algorithm cost.

## Complete product results

All values are milliseconds per measured sample. `tab_click` sums two complete
frames. The ImGui column is the separately measured final product reference.

| Panes | Case | egui before | egui final | Reduction | ImGui final | Final egui / ImGui |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 16 | `idle` | 0.179 | 0.166 | 7.49% | 0.017 | 9.9× |
| 16 | `tab_click` | 0.609 | 0.579 | 4.90% | 0.034 | 17.1× |
| 16 | `surface_resize` | 0.284 | 0.268 | 5.63% | 0.017 | 16.0× |
| 128 | `idle` | 2.240 | 1.526 | 31.90% | 0.138 | 11.1× |
| 128 | `tab_click` | 6.961 | 5.348 | 23.18% | 0.262 | 20.4× |
| 128 | `surface_resize` | 3.437 | 2.691 | 21.71% | 0.132 | 20.4× |
| 1024 | `idle` | 75.742 | 31.025 | 59.04% | 1.098 | 28.2× |
| 1024 | `tab_click` | 174.477 | 83.109 | 52.37% | 2.202 | 37.7× |
| 1024 | `surface_resize` | 89.121 | 43.177 | 51.55% | 1.105 | 39.1× |

| Panes | Case | Before p95 | Final p95 | Before max | Final max |
| --- | --- | ---: | ---: | ---: | ---: |
| 16 | `idle` | 0.198 | 0.330 | 0.208 | 0.597 |
| 16 | `tab_click` | 0.764 | 0.684 | 1.164 | 0.786 |
| 16 | `surface_resize` | 0.329 | 0.341 | 0.533 | 0.430 |
| 128 | `idle` | 2.651 | 1.716 | 2.968 | 2.075 |
| 128 | `tab_click` | 7.617 | 5.856 | 7.797 | 5.911 |
| 128 | `surface_resize` | 3.998 | 3.164 | 4.135 | 3.318 |
| 1024 | `idle` | 79.429 | 31.885 | 82.872 | 38.153 |
| 1024 | `tab_click` | 182.522 | 84.287 | 183.575 | 84.464 |
| 1024 | `surface_resize` | 91.901 | 44.240 | 95.321 | 50.053 |

All nine p50 values improve in this run, but small-layout tails do not uniformly
improve. In particular, 16-pane idle p95/max rise from 0.198/0.208 to 0.330/0.597
ms. Preserve that regression rather than treating the medians as a latency bound.
At 1,024 panes the final product is still 28.2–39.1 times the ImGui reference.

## Method and preserved work

The [unchanged product contract](../../integration/product-comparison/README.md)
uses one context/surface, all 16/128/1,024 panes visible, two tabs per pane,
balanced alternating splits and real content text plus an 8 by 8 square.
Measured samples produce complete CPU draw data with ordinary product styling.
An idle/resize sample is one full frame; a click sums real press and release
frames after two untimed geometry/hover preparation frames on both products.
Timers stop with complete draw data live. Validation, counting and disposal
occur outside each timer. egui repeat passes and ImGui's hidden pre-layout
content remain enabled and recorded.

The fresh baseline uses the byte-verified frozen executables from the
[previous qualification](2026-11-05-product-comparison-results.md). Their original
build commands, compiler/source hashes and exact executable digests are retained
in the new baseline environment; they were not rebuilt from a modified tree.
Subsequent conditions use the serial comparison runner with 24 samples, six
warmups and egui/ImGui/ImGui/egui order. Each condition has 864 raw rows, 18 groups
and 48 pooled samples per product/case, with nearest-rank p50/p95 and maximum.
The Rust driver's only change is a case selector outside Harness construction
and timed execution, to isolate profiles. The runner still selects all cases;
the C++ driver and pinned ImGui inputs/executable remain unchanged.

No Cargo, test or sampler started by this session overlaps formal timing.
The Apple M4 desktop and power settings are not isolated. Sampling is a separate
process, usually eight seconds of a selected workload including readiness,
warmup, untimed diagnostics and output disposal. Symbol inlining and sampling
variability limit attribution; zero
standalone layout frames on cached idle do not establish zero layout-related
time. No GPU, native presentation, input-latency or heap/RSS ratio is measured.

## Attribution

Percentages below are main-thread CPU samples, not fractions of an exact frame
timer. Each category sums disjoint matching call-tree roots; categories overlap
one another. In particular, measurement includes compilation/validation, and
product painting includes auditing. Do not add those categories together.

| Sampled condition | Thread samples | Audit | Layout solver | Scene compiler | Scene validation | Measurement/resources | Tessellation |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Previous qualified product, mainly idle | 6,461 | 81.33% | No standalone frame | No standalone frame | Not separately classified | 2.31% | 1.39% |
| Borrowed-after + junction changes, isolated resize | 5,923 | 67.50% | 1.28% | 5.93% | 5.91% | 18.74% | 1.45% |
| Galley comparison reuse, isolated idle | 5,885 | 64.33% | No standalone frame | No standalone frame | 0.03% | 5.35% | 3.02% |
| Galley comparison reuse, isolated resize | 5,895 | 47.21% | 2.04% | 9.48% | 9.55% | 30.26% | 2.04% |
| Final ownership implementation, isolated idle | 5,935 | 62.02% | No standalone frame | No standalone frame | 0.03% | 6.25% | 3.29% |
| Final ownership implementation, isolated resize | 5,954 | 43.97% | 2.18% | 10.09% | 10.13% | 32.47% | 2.22% |

After galley comparison reuse, begin snapshots still cover 26.97% of idle samples
and 20.22% of resize samples; completion checks cover 27.99% and 19.85%.
Snapshot destruction adds 6.05% and 4.73%. These are disjoint roots within the
audit category, rather than extra categories to add to its total. This evidence
selects the next snapshot-buffer ownership experiment.

## Algorithm changes

The runtime junction-axis query previously scanned all junctions, allocated and
sorted up to two arm IDs, then scanned all splitters for each arm. Calling it
twice for every junction gave Θ(J² + J×S) lookup work, including idle. Runtime
plans already contain validated, unique canonical arrays, so the query now
binary-searches junctions and splitters and handles the two distinct arms
without a temporary Vec. Foreign wrappers still resolve their ID in the current
plan and use its current axis/policy. The query work becomes
O(J log J + J log S), without a new index or authoritative graph.

The changed-scene compiler also scanned all splitters for each derived
junction. A call-local BTreeMap of ID to operability replaces that join with
O(S log S + J log S). Entry insertion retains the first duplicate ID, matching
the original find operation. Candidate derivation, independent scene validation,
region checks, output order and partial failure mutations remain intact. This
changes the join; it does not establish linear complexity for the whole compiler.

No repeated full root projection was found on the unchanged path. The minimum,
maximum and top-down layout traversals have different responsibilities.
Multiple tape resolves and title/resource preparation still occur per paint;
they remain attribution targets rather than grounds for removing checks.

## Audit and immutable-data changes

A Boolean append check now borrows live known lists instead of building and
dropping another complete owned snapshot. Unknown-layer detection temporarily
takes only existing known lists, clones the residual, restores original slots
with RAII and rejects nonempty unknown output. Missing versus existing empty
slots remain unchanged. The complete legacy predicate still runs in its
original order: unknown rejection, current prefix, other-layer roster length,
then each layer's identity, transform and complete shape sequence. List lengths
are checked before element equality. The original owned surface proof and its
terminal output-hook checks remain separate and intact.

The paint-local galley comparison holds a strong Arc and memoizes the original
self-equality result for that allocation. Same-owner comparisons reuse that
exact result; different owners use the original value equality. NaN can produce
false, or an upstream debug assertion can panic; the implementation preserves
both and caches only a normally returned bool. ClippedShape and TextShape fields
retain their original short-circuit order, including nested Vec length checks.
Other Shape variants retain their original equality.

The memo is dropped within the paint call. It can alter temporary Arc counts
and COW/allocation behavior; no unchanged-heap or application-strong-count claim
is made. Pinned Galley data is recursively immutable, which is essential to
reusing self-equality rather than interpreting pointer identity as a value proof.

## Snapshot ownership

The final implementation reuses before-snapshot buffers but still observes
every live descriptor at each callback boundary. Exact raw-bit identity permits
Noop reuse; Text additionally needs the same immutable Galley owner. Changed or
unsupported descriptors are cloned from the live list.
Every begin refreshes layer metadata and checks unknown output. This is a new
capture using recycled storage, not a claim that an earlier capture stayed valid.

Trusting a closed append interval was rejected. Widget plugins, experimental
theme callbacks and Context aliases can modify earlier graphics between pane
callbacks: cached A, external write B, then callback restore A would fool a blind
A-plus-tail cache while the old B snapshot rejects it. Pre-comparing with ordinary
PartialEq is also wrong because begin previously cloned without triggering NaN
equality assertions. Non-panicking sufficient descriptor identity plus live
clone fallback addresses both cases without maintaining a full epaint evaluator.

Opaque payloads must be released outside the Context lock at their original
boundary. Failed checks retain no snapshot. Successful recycling keeps only Text
owners already held by the existing galley memo and Noop slots; all other
payloads are released in current-layer/other-layer and Vec order before the
buffer enters the cache. The cache is declared after the memo so it is destroyed
first on success, error or unwind. These constraints prevent callback destructor
reentry deadlocks, changed opaque Drop timing and additional Weak lifetimes.

The first buffer-reuse version did not improve the 1,024-pane result:
33.801/89.582/46.345 ms became 34.086/89.791/46.191 ms for idle/click/resize.
Its recycling stage accounted for 15.80% of idle samples, including about 7.3%
in repeated owner hashing. The final internal `finish_reusing` operation closes
the complete check, aggregation and recycling. When a successful complete check
compared every Galley with the same owner, each before-Text owner is already in
the memo. That result avoids a second owner lookup. Any different-owner
comparison takes the original per-Text ownership lookup path. The private probe
resets on every finish, and an already-false aggregate never skips the check.

Four timing-only processes compare the frozen galley-reuse and final executables
in earlier/final/final/earlier order, with 48 samples and six warmups per case at
1,024 panes. No build, test or sampler overlaps them. All 576 rows are retained.

| Case | Earlier 1 p50 | Final 1 p50 | Final 2 p50 | Earlier 2 p50 |
| --- | ---: | ---: | ---: | ---: |
| `idle` | 34.042 | 30.554 | 30.956 | 33.921 |
| `tab_click` | 88.250 | 82.730 | 83.379 | 89.968 |
| `surface_resize` | 45.353 | 43.206 | 43.430 | 46.750 |

Both final process medians are lower than both earlier medians in every case.
This supports the observed additional improvement without defining a future
tail bound. This version confirmation measures egui only; the ImGui values above
come from the complete product comparison.

## Duplicate-layer repair

Review also found an integrity gap in the preceding snapshot-move optimization.
A duplicated current LayerId could take the same list twice, overwriting the
captured current prefix with an empty list. A callback could then change earlier
geometry while a zero-length prefix check passed. Restoring a unique roster
before terminal capture could hide that intermediate history from the final
surface proof.

All three capture/check paths now reject any duplicate LayerId before extraction
or reuse. They do not deduplicate or fall back to the vulnerable capture. Normal
layer order remains intact, and one-layer validation allocates no set. Three
regressions drive the actual helpers with duplicated current/other layers,
including unique outer capture, duplicated pane capture, prefix mutation and
restored terminal roster: pane validity stays false even if the outer span still
exists. A malformed duplicate roster intentionally becomes unavailable.

## Evidence and qualification

- [Fresh frozen baseline raw samples](2026-11-05-product-cost-attribution/fresh-baseline/raw.csv), [summary](2026-11-05-product-cost-attribution/fresh-baseline/summary.csv), [environment](2026-11-05-product-cost-attribution/fresh-baseline/environment.json)
- [First candidate raw samples](2026-11-05-product-cost-attribution/candidate-one/raw.csv), [summary](2026-11-05-product-cost-attribution/candidate-one/summary.csv), [environment](2026-11-05-product-cost-attribution/candidate-one/environment.json)
- [Galley-reuse candidate raw samples](2026-11-05-product-cost-attribution/candidate-two/raw.csv), [summary](2026-11-05-product-cost-attribution/candidate-two/summary.csv), [environment](2026-11-05-product-cost-attribution/candidate-two/environment.json)
- [Initial buffer-reuse raw samples](2026-11-05-product-cost-attribution/candidate-three/raw.csv), [summary](2026-11-05-product-cost-attribution/candidate-three/summary.csv), [environment](2026-11-05-product-cost-attribution/candidate-three/environment.json)
- [Final raw samples](2026-11-05-product-cost-attribution/candidate-four/raw.csv), [summary](2026-11-05-product-cost-attribution/candidate-four/summary.csv), [environment](2026-11-05-product-cost-attribution/candidate-four/environment.json)
- [Interleaved version confirmation](2026-11-05-product-cost-attribution/confirmation/raw.csv), [summary](2026-11-05-product-cost-attribution/confirmation/summary.csv), [commands and frozen executable digests](2026-11-05-product-cost-attribution/confirmation/environment.json)
- [Raw profiles, commands and disjoint-root attribution](2026-11-05-product-cost-attribution/profiles/)
- [Frozen final source inventory](2026-11-05-product-cost-attribution/candidate-four-source/sources.json); archived source filenames append `.txt`
- [Frozen-run and profiling reproduction helpers](2026-11-05-product-cost-attribution/tools/)
- [Serial qualification commands and results](2026-11-05-product-cost-attribution/validation/results.json), [554-input source inventory](2026-11-05-product-cost-attribution/validation/source-inventory.json), [log and compressed-content digests](2026-11-05-product-cost-attribution/validation/provenance.json)

Independent crossed reviews recompute every final-candidate statistic and
verify exact per-sample frames, passes, content/visible counts, vertices, indices
and draw commands against the fresh baseline. All are unchanged except timing.
Fifty-one focused proof/junction tests pass, including frozen old-oracle
comparison, raw-bit/NaN behavior, arbitrary between-callback changes, current-plan
membership, duplicate-layer rejection, Weak lifetimes, callback Drop reentry and
panic cleanup. Final serial gates use one Cargo build job and one nextest worker,
reuse existing targets, and pass on the same measured source:

| Gate | Result |
| --- | --- |
| Workspace, all features/all targets | 2,121 passed |
| Independent host, default | 292 passed, including all three 10,000-depth cases |
| Independent host, persistence | 332 passed; already-covered depth binary filtered |
| Official egui consumer | 24 passed |
| Product egui, default / persistence, all targets | 114 / 119 passed |
| Fork-backed native, all features/all targets | 556 passed; one pre-existing ignored test |
| Workspace/native/product correctness and suspicious Clippy | Passed |
| Strict core, egui and native documentation | Passed |
| Workspace doctests | Ten passed |
| Workspace/host/product format, diff and core dependency checks | Passed |

Complete logs are adjacent to the gate record; gzip archives preserve exact raw
contents where whitespace at the end would otherwise fail git's text checks.
All measured inputs and both newly added core test modules still match their
frozen hashes after qualification. Physical GUI, GPU, package and OS input
qualification were not rerun.

Repeated descriptor reads and complete per-callback prefix comparisons still
have quadratic aggregate cost. These changes do not supply a paint mutation
revision or remove that structural limit from the public egui interface.
Further reduction needs measured work on that traversal or a backend that can
provide reliable mutation evidence or immutable paint blocks; relying on a
cached prior validity result would change the contract.
