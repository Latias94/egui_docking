# Current-profile location and equal-binary contract review

Findings first: the sampled `complete_item_move_structure +1352` reserve is conclusively the `RootStructure::build_rows` **pending `Vec<BuildFrame>`**, specifically `pending.extend(...)`, rather than `split_rows`. This supports one bounded initial-capacity experiment on the existing standard `Vec`. The equal-binary arithmetic proposal also has a narrow bit-preserving proof, provided every existing validation, aggregate check, infeasible return, clamp and correction remains in its original position. Neither finding establishes a performance improvement.

This is an independent static review. I read the saved current samples, receipts, immutable Git objects and executable; I invoked `xcrun llvm-objdump` only to inspect the executable. I did not build, execute, test, sample or time anything, change repository files, or read another current-round review.

**Evidence binding.** The reviewed diagnostic directory is `/tmp/dockspace-imgui-twofold-20261107-ea4161e/post-main-continuation/current-main-profile/`. Its receipt identifies source HEAD `c7508eec3ac44138305797691244ae9c180d7680`, retained runtime `0e441c8`, and formal executable `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.

| Evidence | SHA-256 |
| --- | --- |
| `receipt.json` | `ec94fad210c92a69b6d0d86060bcf40b592542f7a0857f81353a159fc13a0f01` |
| `diagnostic_docking_cost` | `35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590` |
| `attribution.json` | `7604545a1451db79c66664bedf2f5d8b7b7299cf4f7286042a73bfd509a94962` |
| `16-1-edge_split.sample.txt` | `79920695cbbc186741248857f266916ae23c9f4b6e2050b56ab6240b9375fd4f` |
| `16-4-resize.sample.txt` | `ddc33351a8ac0d947ff1b267584c1da9b436cbed65bc55c2bef9dd7c3d644502` |
| `16-1-center_move.sample.txt` | `a704c5599f34f2f15912c626ffcbb2cbfa3cc9f9568e4fdb2072bca45d3ae242` |
| `graph/root_structure.rs` | `a89bf210b4836d10380007e1d5594ad6c76281a338b12d10f0408ba7ee98eca7` |
| `graph/authority.rs` | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |
| `layout.rs` | `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` |
| `layout/reference.rs` | `4b6ff84c9bed4b65a202e1c771b5f176b161bbb27b99dba25e4875c7b08c07c6` |

Source paths in this report are relative to `crates/dockspace/src/`. I independently hashed all 324 formal source entries from immutable `c7508ee` Git blobs; all match the receipt. Comparing its formal and diagnostic maps gives exactly one differing path, `docking_cost.rs`, the saved iteration-cap change. The receipt records restoration before execution and unchanged diagnostic/formal executables. Current live `operation.rs` differs from that frozen source during this review; it is excluded from this finding, rather than being treated as diagnostic source drift.

**Exact reserve location.** Edge sample lines 1273–1277 have the following inclusive ancestry:

`docking_cost +1828 → prepare_transaction_inner +2592 → complete_item_move_structure +1352 → reserve::do_reserve_and_handle → finish_grow → realloc`.

There is no fixture ancestor. The saved attribution records the same path. Its 30 inclusive reserve samples overlap the descendant counts, including 26 under one `realloc` branch; those numbers must not be added or translated into allocation counts, clock shares or saved nanoseconds.

The Mach-O preferred function address is `0x1009be010`, symbol `complete_item_move_structure::h4b1521e7be6ad768`. The edge sample's load address is `0x10054c000`, giving slide `0x54c000`. Thus sampled return PC `0x100f0a558` maps to `0x1009be558`, function offset `0x548 = 1352`.

The decisive disassembly is:

```text
1009be540: sub x0, x29, #0x78
1009be544: mov x1, x21
1009be548: mov x2, x22
1009be54c: mov w3, #0x8
1009be550: mov w4, #0x28
1009be554: bl  reserve::do_reserve_and_handle
1009be558: ldp x20, x21, [x29, #-0x70]
1009be55c: b   0x1009be46c
```

The same local descriptor at `[x29 - 0x78]` is initialized with capacity one and a 40-byte allocation at `+328..+368`, popped with a 40-byte stride at `+432..+456`, and receives an `Exit` tag of two before child extension. The resumed child loop at `0x1009be480..0x1009be49c` writes the child slot, child key, parent key and index with a 40-byte stride. This identifies `BuildFrame` (40-byte elements, alignment eight in this executable), including the parent/slot payload; element size alone would not distinguish it from every other 40-byte structure.

By contrast, `split_rows.reserve(edges / 2)` uses the local descriptor at `sp + 0x140`, passes alignment eight and element size eight at call `0x1009be284`, and returns at function offset **632**—a separate one-sample reserve path in the same saved edge subtree. `nodes.reserve` is the distinct 40-byte call returning at offset 1268. The substantial `+1352` path is therefore not evidence for optimizing temporary split ordinals or the directory sorter.

The source mapping is direct: `graph/root_structure.rs:126` initializes `pending` with one `Enter`; lines 184–194 push `Exit` and extend reversed child `Enter` frames. `graph/authority.rs:168–171` runs this builder only for an editing Item-move root. The center sample's two observations of this function are only at offsets 48 and 100, consistent with the ready-root path; this reserve proposal does not target ordinary center completion.

**Smallest faithful capacity candidate.** Change only initialization of that standard `Vec`, retaining the initial `Enter`, all push/pop/extend operations, callback placement, lookups, error returns and directory construction. A single defensible bound is `node_capacity_hint.clamp(1, 16)`: no hint preserves capacity one, while checked Item builds reuse the existing `previous_nodes.saturating_add(2)` hint. This adds no metadata, alternate traversal, persistent scratch or container dependency.

The cap differs from the projection stack's eight for a source reason: this stack retains both ancestor `Exit` frames and unvisited siblings. A balanced 16-leaf binary tree can require nine pending frames; an added wrapper can increase the bound further. Capacity eight therefore cannot remove all growth even in this small shape. Capacity sixteen covers that bounded case without pretending to cap arbitrary depth or arity; ordinary `Vec` growth remains available. This is one experiment, not a recommendation to sweep capacities.

The cost is real: initial element storage can rise from 40 to 640 bytes. A previously large root that collapses to a Tabs root can overreserve because its hint describes previous nodes. Small or shallow shapes can also reserve beyond their eventual need. The change does not remove the initial allocation, other builder buffers, directory sorting, or `Arc` construction. Allocation failure timing can change, as with any reserve adjustment; recoverable graph errors and their order need not change. Old structures remain owned by their previous `Arc`s, and no candidate/rollback/reference protocol is touched.

Appropriate qualification is the existing full ordinary/work/vector protocol, including deep and n-ary topology coverage and staged rollback/old-reference checks. A new test asserting allocator capacity would mirror the implementation. Formal complete-operation comparisons, especially unaffected center/resize and medium/large cases, must decide retention.

**Equal-binary arithmetic: exact boundary.** There is a separate, genuinely different bounded numerical experiment. At the existing desired stage only, after the unchanged validation and minimum/maximum aggregate handling, recognize `central_index == None`, exactly two weights, and equality of the actual `f64` weights. Generate both desired values from one `extent * 0.5`; then execute the original clamp, assigned-size sums, grow/shrink and outcome code. All other inputs retain the generic desired helper. No weight-total return value or extra scalar ABI from rejected candidate one is needed.

For a matching input let both weights be `w`. The unchanged validator (`layout.rs:1264–1305`) has already established finite nonnegative weights, finite full total, and a strictly positive total for this nonempty non-central case. Hence `w > 0`, and `w + w` is finite. Doubling a binary floating-point value is exact when finite, including subnormals; dividing `w` by that exact double gives exactly representable `0.5`. The frozen expression at `layout/reference.rs:759–762`, `extent * (w / total)`, therefore has the same multiplication operands and result bits as `extent * 0.5`. Computing that product once and copying it preserves both desired values.

This proof does not depend on normalized weights. It covers the smallest positive subnormal, the normal/subnormal boundary and the largest weight whose doubling is finite. `[MAX, MAX]` still fails the validator's `"weight"` aggregate before reaching it. Equal signed-zero pairs, including `[+0, -0]`, still fail `ZeroWeightTotal`. Nonfinite/negative weights and length/central-index errors retain their original precedence. `extent == -0.0` is allowed and must use multiplication, preserving the original negative-zero desired bits. Subnormal extents can underflow on halving; the shared correction tail must remain, so no exact-conservation early return follows from the proof.

Central allocation is wholly excluded. Its own weight may be ignored for allocation, but still participates in scalar validation and full-total overflow; central sibling sums/products and `"desired size"` errors cannot be rewritten as total-minus-central. Unequal weights and n-ary input also fall back. The guard must not precede either `"minimum"` or `"maximum"` aggregate checks or infeasible returns (`layout.rs:371–397`); notably a maximum aggregate overflow still precedes the minimum-infeasible return. No constraints, geometry or output fields are omitted.

The most valuable differential cases are equal minimum-subnormal/minimum-normal/large-finite weights; signed-zero and subnormal extents; asymmetric min/max clamps requiring the original correction; both infeasible outcomes; conflicting aggregate/scalar errors; and central/unequal/n-ary fallbacks. Compare every size and outcome by `to_bits()` with the unchanged frozen solver, plus exact error variants/labels and precedence. These are semantic cases for Root's qualification, not tests executed by this review.

**Cheapest executable check, already performed.** In this diagnostic executable, `solve_validated_axis_into::h5a2bf009f4416471` starts at `0x100b39a30`. Its non-central desired sum is at offsets `+1136..+1152`, and the two-child path uses the scalar loop:

```text
100b3a02c: ldr  d1, [x11], #0x8
100b3a030: fdiv d1, d1, d0   ; +1536
100b3a034: fmul d1, d17, d1  ; +1540
100b3a038: str  d1, [x12], #0x8
```

The loop runs once per child; the vector branch requires at least eight entries. Thus equal weights have not already been specialized away in the current binary. The same symbol occurs under timed layout ancestry in the current samples, but the grouped PCs do not justify assigning a division-specific time share. Fixture source (`docking_cost.rs:72–80, 87–103, 147–158`) supplies equal binary splits without a central leaf, including the new 0.5 edge wrapper. It establishes plausible applicability, not how much latency this arithmetic contributes.

Root can reproduce the narrow inspection with `xcrun llvm-objdump --disassemble --demangle --no-show-raw-insn --start-address=0x1009be420 --stop-address=0x1009be580 <diagnostic-binary>` and the solver range `0x100b39e98..0x100b3a044`. Candidate symbols/addresses must be rediscovered after compilation. Added guards and altered code layout can still regress all sizes despite deleting operations; only a separately frozen full comparison can resolve that risk.
