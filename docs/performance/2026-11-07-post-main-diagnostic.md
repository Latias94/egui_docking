# Current-runtime diagnosis after the main merge

The previously qualified `c7508ee` was fast-forwarded from `9e73170` into
`main`, without binding any worktree to main. The new main run
[42363449111](https://github.com/Latias94/egui_docking/actions/runs/42363449111)
passes all six jobs, including the actual editor and physical X11 docking.
The retained runtime at this checkpoint remains `0e441c8`, with 30/42 formal
complete-operation comparisons at most twice ImGui. This diagnostic supplies
new location evidence, not a new performance score or a stopping conclusion.

## Source and sampling boundary

The formal executable is
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.
All 324 formal inputs match this source checkpoint. Reusing the preceding
diagnostic method changes only the probe iteration-cap assertion, from 10000
to 1000000; no operation, fixture, timer, validation or disposal changes.
The original probe is restored and all formal hashes checked before sampling.
The diagnostic executable is
`35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590`.

Each serial case runs 500000 observations and eight warmups. The sampler
requests four seconds at one-millisecond intervals. All three workloads and
samplers succeed. Raw durations remain diagnostic-only; the unchanged formal
ABBA/BAAB protocol decides candidate retention. No cross-machine confirmation
or allocation-time share is inferred.

| 16-pane case | Worker observations | Timed layout inclusive observations | Timed candidate preparation inclusive observations |
| --- | ---: | ---: | ---: |
| Four views, resize | 2904 | 169 | 0 |
| One view, center move | 2925 | 142 | 100 |
| One view, edge split | 2926 | 177 | 218 |

These are outermost named subtrees, not exclusive clock fractions. Fixture
construction, independent postvalidation and returned-result disposal have
different ancestry. Inlined or unresolved frames remain unknown. Independent
review corrected the initial resize classification from 170/9 subtrees to
169/8: one `docking_cost +1652` return belongs to fixture construction despite
its missing intermediate fixture frame. The original classification remains
archived. All 222 saved memory paths and the specific findings below are
unchanged by that correction.

## Concrete remaining work

The current timed Item detach reaches `subtree_has_items` and its temporary
traversal/visited storage. After a successful removal, the source Tabs may
still contain an item; that exact local result can prove the root nonempty
without another existence query. The old query stops at the first nonempty
leaf, so eliminating it is not a claim to eliminate a complete tree walk on
every command. Preserve root lookup, central-root handling and the original
whole-root fallback when the source becomes empty. The separate
[source-nonempty experiment](2026-11-07-source-nonempty.md) owns qualification
and paired results; this profile is of its actual unchanged control.

Static disassembly also binds three distinct locations:

- `complete_item_move_structure +1352` is the 40-byte `BuildFrame` pending
  stack's `extend` reserve. Its 30 inclusive observations are nested, not
  additive allocator samples. It is distinct from `split_rows.reserve` at
  return offset +632. An initial bounded standard-Vec reservation is a
  supported experiment, with real overreservation costs on shallow roots.
- `refresh_child_placements +152` is the return from the temporary children
  roster's allocation. The same function contains memcpy, placement updates
  and free. Borrowing the immutable roster while updating the disjoint map
  can remove that snapshot, but must preserve full generational existence,
  equality checks and lazy `Arc::make_mut`.
- The noncentral numerical executor still contains a per-child division.
  Exactly two equal, accepted weights admit an exact half-share desired
  value after the original validation and min/max checks. This is a separate
  hypothesis from the rejected total propagation and initializer unrolling;
  it must retain original clamp, correction and outcomes and earn retention.

Inside `project_measurements`, offset +1632 allocates split outcomes, not the
continuation stack. Offset +1984 allocates the stack and +6564 disposes it.
Binary gather/axis scratch already uses local arrays. Cross-root scratch
reuse would require an explicit caller migration under a different formal
scope. Compiled traversal events could remove the stack without that caller
change, but add root-completion work, structural metadata, retained thickness
and raw-DAG complexity. It is deferred behind the smaller located deletions;
neither its success nor its impossibility is established.

## Evidence

The [archive](2026-11-07-post-main-diagnostic/evidence.tar.xz) preserves source
maps, the one-line diagnostic patch, build/sampler receipts, three compressed
raw CSVs with 1500000 rows, complete call graphs, both classifications,
source/disassembly reviews, the merge receipt and successful main CI log.
Its manifest is checked by a complete byte-for-byte archive round trip.
The [independent evidence review](2026-11-07-post-main-diagnostic/post-main-diagnostic-evidence-review.md)
establishes the source boundary and the fixture correction. The
[location review](2026-11-07-post-main-diagnostic/post-main-current-location-review.md)
records exact instruction addresses, the pending-stack owner and the
equal-weight numerical argument. None assigns saved nanoseconds to samples.
