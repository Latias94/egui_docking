Placement-roster borrow — cheapest static location and minimal design, 2026-11-07

The temporary children Vec clone is present in the exact diagnostic binary and actually executes under the saved timed edge preparation. It is a supported small follow-up to investigate after source-nonempty's decision. A borrow of immutable node storage alongside mutable placement storage can remove it while retaining the current lazy COW predicates. No implementation, build, test or performance result is supplied by this report.

The baseline distinction is explicit. Saved profile source is immutable c7508eec3ac44138305797691244ae9c180d7680, runtime 0e441c8, formal binary `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`. Its iteration-cap-only diagnostic binary is `35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590`. Source-nonempty is the current pending candidate, not yet a retained/qualified control in this design review. Its final operation.rs snapshot is `bf81e49acdf0678cbb25cdf1f769fa9e465a45a9034dd349342398f839eee349`; it changes operation.rs only and leaves authority.rs unchanged. The saved profile is not called a profile of that candidate, and this analysis does not stack a new treatment onto it before Root decides.

Only this report was written. I read the existing edge sample, source and receipt, and statically inspected the same diagnostic executable with LLVM objdump. No binary/workload was executed, no sampling or timing was started, no Cargo/build/test/network operation was run, and no candidate/repository file was modified. I did not read another agent's report. The saved diagnostic binary was independently hashed before inspection.

At `graph/authority.rs:282–292`, refresh_child_placements first checks the parent's actual placement, reads its Split children, clones their Vec and iterates the owned copy to call place_node. This clone is a raw Vec<NodeId> copy; it does not pass through Node::clone. Nothing in the loop mutates node storage or the child roster. place_node only tests the child's full generational existence, compares root/parent/index in the separate placements map, then makes that map mutable on an actual difference. Splitting immutable storage and mutable authority borrows is therefore sufficient; the clone is not protecting an arena mutation.

I used `/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/llvm-objdump` in symbol-table, bounded-disassembly and Mach-O indirect-symbol modes. No disassembler output file was created. The exact refresh_child_placements symbol has linked address 0x1009bded4 and hash suffix hb8afa1e0e0e490bb. Relevant disassembly:

| Linked address | Symbol offset | Meaning |
| --- | ---: | --- |
| 0x1009bdf50–0x1009bdf54 | +124 through +128 | Load child length; return if zero |
| 0x1009bdf5c | +136 | Load original children buffer |
| 0x1009bdf60 | +140 | Compute children.len() × 8 |
| 0x1009bdf68 | +148 | Call stub 0x100c91618, resolved to malloc |
| 0x1009bdf6c | +152 | Allocation return PC and allocation-failure test |
| 0x1009bdf7c | +168 | Call stub 0x100c91630, resolved to memcpy, with the same byte length |
| 0x1009bdf8c–0x1009bdfb0 | +184 through +220 | Read cloned NodeIds and call place_node in index order |
| 0x1009bdfd0 | +252 | Tail call stub 0x100c91588, resolved to free |

The allocation, memcpy, per-child loop and free remain in optimized generated code; this is not an optimized-away source clone. Zero-child/non-Split/missing-parent paths can return before it. NodeId occupies eight bytes in this executable, as the length shift and loop stride show; this is not asserted as an unconditional portable ABI.

The saved `16-1-edge_split.sample.txt` independently ties that code to the timed path. At lines 1109–1111 it enters docking_cost +1828 → prepare_transaction_inner +864 → apply_command. Its insert_payload +2420 subtree reaches refresh_child_placements +212 at line 1128, with seven inclusive observations below the per-child place_node call. Later, the same timed docking_cost ancestry reaches prepare_transaction_inner +2552 at line 1341, then:

- line 1342: refresh_child_placements +60 → placement, two inclusive observations;
- lines 1346–1347: refresh_child_placements +152 → allocator, one inclusive observation;
- lines 1348–1351: refresh_child_placements +212 → place_node, one inclusive observation.

The edge process has slide 0x54c000. Sampled +152 address 0x100f09f6c minus that slide is exactly linked return PC 0x1009bdf6c, following malloc at +148. Sampled +212 likewise maps to the return following place_node at +208. Thus the allocator observation belongs to this children clone, distinct from Node payload COW, placement-map COW, node-store growth or root-row reserve. Fixture subtrees elsewhere are excluded by this explicit timed preparation ancestry. The sample does not resolve every allocation or provide allocation frequency/time; one observed allocator stack is sufficient to establish execution, not a benefit magnitude. Nested inclusive samples must not be summed or converted into a nanosecond share/lower bound.

The smallest isolated implementation design changes only refresh_child_placements's body:

1. Keep the original parent placement read and return behavior first. Keep the following immutable Split lookup and its return behavior.
2. Borrow the existing node store immutably for the whole loop, including its children slice. Separately borrow only authority.placements mutably. These are disjoint Workspace fields; no Node::get_mut, Arc::clone of the Split payload, copied child Vec or whole-Workspace mutable helper call is needed.
3. Iterate the borrowed children in the same order and copy each NodeId scalar. For each index, preserve the exact child `nodes.contains_key` gate, desired NodePlacement root and ParentLink values, and equality comparison with the current map entry.
4. Invoke Arc::make_mut on placements only inside the inequality branch, immediately before the same insertion. Do not acquire a mutable SparseSecondaryMap once before the loop.
5. Keep the original Workspace::place_node and every other writer unchanged for this first isolated experiment. The existence/equality/update sequence is only a few lines; reproducing that exact local sequence avoids a general helper/provider refactor or changing all place_node callers' code generation as part of this treatment. If later maintenance warrants extraction, a private setter taking the concrete node store and placements fields can share the same body, but it should not expand this experiment.

This is a design, not a Rust patch or a compiler-checked implementation. Field borrowing is available because node storage does not change while only the placement map is mutated. The update stays within the existing graph authority; no second validator, public trust bool, token, count cache or new container is introduced. A stable parent roster needs no owned snapshot when the only writes are to a different map.

Lazy COW is a required semantic/cost boundary. The original place_node at 273–279 first checks the actual node, then compares the complete NodePlacement, and calls Arc::make_mut only when the entry differs. An eager Arc::make_mut before iteration would detach the entire shared SparseSecondaryMap even for a zero-change refresh or a roster containing only missing children. That would defeat the intended deletion and change the current ownership behavior. The borrowed version must retain all three gates, including full generational node existence; equality in a secondary map must not authorize a forgotten/recycled arena node. It must retain the parent's original placement lookup before the Split inspection. No new error, panic or missing-child assertion should be introduced.

Both refresh call sites should remain. During the representative inner-edge wrapper insertion with nonempty source, operation.rs:3472 refreshes the new wrapper and canonical.rs:327–328 refreshes its final output frontier again. They have the same two children when no source collapse/outer flattening intervenes. The clone currently executes on both calls even if every final placement already agrees. Removing the temporary clone is safe independently of whether either refresh is redundant. Deleting one refresh globally would require a different proof across source collapse, outer flattening and retired nodes, and is not part of this design.

For the specific two-child wrapper on this build, each existing call requests 16 bytes for the cloned NodeIds and copies those 16 bytes, then frees the temporary. Two sequential refreshes therefore have two such clone allocation/copy/free sequences. This is a source/disassembly count for that branch, not a measured allocator-time or product-wide allocation claim. It does not remove the real placement-map COW needed by an edge edit, Node payload COW for the changed parent, root structure rebuild/Arc sharing, or output buffers. Central/nary and raw/missing-child behaviors continue through the same placement predicates.

The existing logical work CSV should remain byte-identical. `drop_resolver.rs:1210–1233` explicitly excludes arbitrary Vec clones, allocator events and placement-directory copying. `record_node_payload_clone` at 1306–1320 increments split_child_copies and split_weight_copies only for a concrete Node::clone. refresh_child_placements's independent children.clone is not intercepted there. Removing it therefore must not decrement the ledger's split_child_copies, add a fictional zero-copy claim to the counters, or weaken their assertions. Expected unchanged columns include transaction candidate/fork, node handles/payloads, tab/MRU copies, split-child/weight payload copies, completed-root rows/handles and all item/fingerprint counters. In particular the normal edge ledger's two split-child and two weight copies remain due to the real parent payload COW.

The cheapest next verification is the existing source-pair review plus ordinary work/placement contracts after Root authorizes a concrete candidate. The saved binary and sample already establish this location, so another profile is not necessary to justify attempting the bounded deletion. Keep semantic_edits_share_placements_and_discarded_structure_edits_detach_them, local_item_moves_match_complete_normalization_and_exact_mru with assert_actual_placement, last-item retirement, source-collapse/outer-flatten error/rollback, exact refs/ABA and owned-output tests. A new test that only asserts “children.clone was not called” would mirror implementation. If existing ownership coverage lacks an already-equal shared placement refresh, one small semantic assertion in the established fixture can verify it remains shared; do not add a benchmark framework or a second placement validator.

Qualification and complete performance measurement would still belong to Root, with the actual parent selected only after source-nonempty's outcome. If source-nonempty is retained, apply only the one authority hunk onto that exact parent, bind full source pairs/new binary and compare against that actual frozen control. If it is rejected, use the restored retained control. The old c7508ee diagnostic supports this unchanged source location, but does not certify a new parent's emitted code or predict its paired score. Code-size/inlining/call lowering can offset a small allocation deletion. Benefit remains unknown.

| Read identity | SHA-256 |
| --- | --- |
| graph/authority.rs, unchanged c7508ee | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |
| graph/shared_storage.rs | `18c6f75150643779acb9c71c30486634b19f7ccec5677e4cc64ce9043fcc532e` |
| canonical.rs | `b2f1ab4bd5b7e5933966c54792b30f4d257f823f172a75c8686f9260af0ea517` |
| drop_resolver.rs, counter semantics | `f8b093f247417b4abe2a79dc13febb0549bf214066c4bc9dc0739a72e2579a7f` |
| Current-main diagnostic receipt | `ec94fad210c92a69b6d0d86060bcf40b592542f7a0857f81353a159fc13a0f01` |
| Saved edge sample | `79920695cbbc186741248857f266916ae23c9f4b6e2050b56ab6240b9375fd4f` |
| Bounded objdump stdout, 0x1009bded4 through 0x1009be074 | `b81ba0eddfcf4728183c1b4055e49497cf3dbc6f15b1e482bcd46b0d9eb6db14` |

The evidence is sufficient for a small source-backed follow-up design, not for adopting it or changing the current pending candidate's status. No source file or other report was written by this task.
