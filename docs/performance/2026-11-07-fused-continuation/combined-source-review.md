# Independent delta review: fused continuation on retained Vec

**Source go.** No source blocker was found. The frozen base equals immutable `eb94cb242722962c9e0d88074d81cf462d784357` byte for byte, and the candidate applies only the original two fused continuation transitions. It is suitable for Root to apply and qualify as an independent slice. This is source approval, not compilation, behavioral qualification, retention, or a performance claim.

Only this report was written. No Cargo, compilation, formatting, tests, runner, timing, profiling, network, delegation, or repository/fork/cache/candidate modification was performed.

| Artifact | Independently checked SHA256 |
| --- | --- |
| `manifest.json` | `115987d81ab70356143b814efcba08efa316f57a072c5e82519415b59969299d` |
| `candidate.patch` | `62b0fc849e0e465af9913d965cf2dd28efa3cc4be64ad7ba2d9e54a3cf28dc46` |
| Complete base `layout.rs` | `ee0bbdadf60b10a3787539e89cdee41411301c3c774e66d53c0146ad1567eaec` |
| Complete candidate `layout.rs` | `877ffb64315a26932ab8a617761098bc1168929cac34f5420174f511398f40d9` |

The manifest identities match actual bytes. The complete base matches its immutable Git source. Independently applying the two intended transformations and independently reconstructing the unified patch both produce the complete candidate exactly: one file, two hunks, +1/−3. Reversing precisely the three SmallVec edits in the previously reviewed `/tmp/fused-continuation-current/` candidate produces this same tree. No SmallVec import/type/macro remains.

The rebase introduces no axis-validation specialization, numerical change, persistent directory, dependency/configuration change, or new container. The stack is the retained standard `Vec<ProjectionFrame>` with its original remaining suspension/Enter pushes and ordinary unbounded growth.

The first edit immediately calls the existing advance helper after the unchanged Enter preparation succeeds. The original pushed a ContinueSplit frame solely to pop and dispatch it next. No other frame, reader callback, publication, or semantic hook intervened. Preparation, solver dispatch, central handling, and release prefix guard remain at their original positions.

The second edit falls through only after the original pending-child work. Candidate lines 1185–1227 still order events as descendant completion, parent-specific extent/cursor update, fallible nonfinal gap construction, `following_gap = Some(splitter)`, thickness update, then completion or next-child cardinality/bounds construction. Descendant errors still precede parent gap errors; gap errors still precede the next child bounds/preparation. Final children create no following gap. The removed repeated helper prefix only reread immutable topology and the same NodeMap bounds; the intervening gap-field write changes neither.

Packed minimum/maximum/extent/Option-gap storage, all initializations and size copies, checked solver arithmetic, output ownership, and failure discard are unchanged. Actual Enter order and parent suspension order remain identical, preserving raw DAG revisits, parent-edge results, node/split last writes, and every per-Enter prefix assertion. The helper schedules descendants without native recursion, so deep traversal retains the same dynamic stack behavior. Reference/test sources and complete input/export/timer scripts receive no hunk.

The prior semantic review therefore carries across this exact container restoration. It does not carry a SmallVec retention decision: that rejected experiment is absent here. The deleted Vec pushes also cannot allocate, since each restores a just-popped height without reducing capacity. Any benefit or regression concerns control-flow/code-generation effects and requires Root's ordinary semantic gates and unchanged complete-operation comparison.
