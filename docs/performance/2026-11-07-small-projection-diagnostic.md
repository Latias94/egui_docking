# Current small-root projection diagnosis

The diagnosed runtime is immutable `49baba1`, whose qualified formal binary is
`6d255458eac25cd9dcf629b88cfca6959c78db7ed2bb7ccf08fe2cba6332dcfd`.
This diagnostic locates an avoidable traversal-stack expansion and motivates
one bounded follow-up. It supplies no new formal score, allocator-time share or
latency lower bound.

## Coverage and binding

The initial attempt runs that unchanged formal binary with its 10000-sample
cap. Both probes finish in a fraction of a second: resize has no call graph and
edge has only 107 worker samples. Those recordings remain archived and do not
support a stable attribution or stopping decision.

For the longer diagnostic, exactly one assertion in `docking_cost::setting`
changes its cap from 10000 to 1000000. The operation, fresh inputs, validation,
fixture and destruction remain unchanged. A serial production-profile build
freezes independent diagnostic executable
`b6c2b06483361b118d22c916ef0f06e1f9f8546d4d3d80eea11bf914a587e900`.
Before either run, the script restores the original probe and verifies all 324
formal source hashes. The formal binary is unchanged. The extra loop range is
exclusive to this diagnostic; its durations never enter formal comparisons.

Both selected cases use 16 panes, 500000 measured observations and eight warmups.
The workload and sampler succeed. Each workload lasts approximately ten seconds;
`sample` requests four seconds at one-millisecond intervals. The recordings
contain 2905/2941 worker-thread observations, respectively. The waiting main
thread is excluded. The requested interval does not guarantee exactly 4000
samples or an exact elapsed CPU duration.

| Case | Worker observations | Timed layout/leaf-read subtree observations | Timed candidate-prepare subtree observations | Timed stack-growth subtree observations |
| --- | --- | --- | --- | --- |
| 4 views, resize | 2905 | 193 | 0 | 25 |
| 1 view, edge split | 2941 | 175 | 218 | 18 |

The last column is nested within layout, not additive. The classifier counts
only outermost matching call subtrees, excludes `fixture` ancestry and leaves
unresolved/inlined direct probe code unclassified. Initial fixture layout,
independent postvalidation, fixture destruction and returned rich-result
destruction are outside the formal operation. Global allocator summaries and
validation calls in those branches cannot be charged to timed mutation.

## Exact stack-growth site

Both call graphs contain the following path outside fixture ancestry:

```text
docking_cost::docking_cost
  project_all_roots
    layout::project_measurements + 4260
      RawVec<T>::grow_one::h370c7ae3627027d4
```

The retained source initializes the iterative projection stack with
`vec![ProjectionFrame::Enter { row: 0, bounds }]`. The diagnostic disassembly
binds the sampled return offset 4260 (`0x10a4`) to the call at
`0x100b388a8`, returning at `0x100b388ac`. The specialization passes element size
48 and alignment 8 to the existing grow helper. The following stores write the
Enter discriminant, node row and four rectangle coordinates at a 48-byte stride.
This identifies continuation storage rather than the leaf-input or durable
result buffers. Addresses and ABI size describe this diagnostic build only.

One follow-up is justified: retain standard `Vec` and every traversal operation,
but initially reserve `min(root_node_count, 8)` slots before pushing the same
first Enter frame. This avoids the observed early expansion in the balanced
small fixtures while deeper trees still grow normally. It retains one heap
owner per root, and can increase temporary capacity on shallow roots. The
source review and complete-operation measurements must decide retention.
The already rejected SmallVec and fused-transition variants do not establish
the outcome of this separate reservation change.

## Evidence

The associated archive preserves both short failed-coverage attempts, long
receipts and compressed raw observations, source/diagnostic probe pair, exact
one-line patch, build and sampler logs, complete call graphs, disassembly,
classifier and source-scope review. It binds runtime identity and pre-run source
restoration. No extra clocks are added to the formal operation.

The [independent evidence review](2026-11-07-small-projection-diagnostic/independent-evidence-review.md)
finds no blocking source, coverage or location issue. Its SHA-256 is
`be726a068485856b7627ebab27610422d7bf860fc5d348310bbbe436b0247c2b`.
The [archive](2026-11-07-small-projection-diagnostic/evidence.tar.xz) preserves
the complete inputs with a per-file hash manifest. The reservation follow-up
still requires ordinary qualification and full paired measurements.
