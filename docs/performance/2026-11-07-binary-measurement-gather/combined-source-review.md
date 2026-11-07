# Independent source review

No substantive Rust source blocker found. The candidate changes temporary storage at two gather sites while retaining the original composers, ordered inputs and error boundaries. This is a static review, not compilation, test qualification or evidence of a performance benefit.

## Bound source

- Immutable parent: `eb94cb242722962c9e0d88074d81cf462d784357`.
- Patch SHA-256: `8db3c0cf34440a48be5563844a872e7e8d36fdf75bacd0d7c908d4b3ea19b7d2`.
- Manifest SHA-256: `64cb72f102e579415acdad6fc6d6a027bd422e9c7eca53df7968efacc237fb2d`.
- READY SHA-256: `5a1df199a894a4bcb1a9464e6d45ff4f113c3ef821d7ae919b32ea9da42961ff`.

| Complete source pair | Base SHA-256 | Candidate SHA-256 |
| --- | --- | --- |
| `crates/dockspace/src/layout.rs` | `ee0bbdadf60b10a3787539e89cdee41411301c3c774e66d53c0146ad1567eaec` | `d28a55c27baeffbedc27ecadf2f1a713d7aed14ff2e501b777e9aea595d0b03d` |
| `crates/dockspace/src/layout/topology.rs` | `1dd27eb635c84e0d73bf9186eb2e741f400b10e4b36bb6365477ac01c5bd21c6` | `ab10ff27f57c3f03a2336daec731999af7d9c6038076cf4820d4ddf6a461fa44` |

Both base files match their immutable Git blobs. Independently applying the two unified hunks in memory reconstructs both complete candidate files exactly: two runtime paths, two hunks, **+55/-15**. The unchanged reference, topology tests, override tests and docking probe also match the immutable parent. Artifact digests and the declared UTF-8 path-string-ordered source-set digests were checked independently; the author's scope-check results were not treated as qualification.

## Semantic findings

At `tree/crates/dockspace/src/layout/topology.rs:162`, exactly two ordered child rows produce two local `SubtreeMeasurement` records. Their Constraints are copied first-child then second-child, matching the original iterator. The original `compose_constraints` receives the same ordered slice contents once, at the same point. Postorder traversal, fresh leaf calls, central-leaf selection and subsequent row assignment are unchanged.

At `tree/crates/dockspace/src/layout.rs:620`, the same storage choice occurs inside the existing minimums-only delayed maximum pass. It retains the original `compose_maximum`, `total_splitter_extent` evaluation and subsequent `Constraints::new` boundary. Maximum composition is not advanced into measurement or discard. Override validation and earlier measurement failures retain their existing precedence.

Every other arity keeps the original `Vec::clear` followed by ordered `extend`. A binary split can leave an earlier general split's scratch contents untouched, but only its local array is borrowed during that call. The next general split clears the Vec before use. There is no assumption that a root is wholly binary, that children are unique or that scratch starts empty.

The local records own copied Constraints; they do not borrow `values` across the later parent-row write. Repeated child rows still produce separate entries in the composer input. Both records retain the original production gather's `contains_central: false`; the composers read Constraints, while raw central propagation remains in its existing checked compiler. No input/output reuse or callback purity assumption was added.

The checked raw compiler's traversal and gather are unchanged, including cardinality, missing-node, cycle, completed-subtree reuse and central diagnostics. Its successfully compiled projection still reaches the same maximum-composition site with equivalent ordered records. The slice-based composer bodies, arithmetic reductions, checked-sum labels and Result-operation order are byte-identical. No solver, continuation, packed-output field or geometry-error boundary changed.

The unchanged existing mixed-arity reference comparison exercises two-, three- and four-child trees through full and delayed projection. Together with the existing reader/override precedence, raw DAG, central-depth, late-error and old-output witnesses, it provides an appropriate qualification surface. This review did not execute those tests.

## Cost boundary and disposition

For a wholly binary pass, the temporary child Vec is no longer populated. The old Vec was reused across splits, so this does **not** establish one allocation saved per binary split. Empty Vec construction remains. Both records and the same Constraints copies still materialize; mixed/general roots retain the Vec and its capacity.

The extra arity branch and local arrays may alter register pressure, stack use and code generation. No measured allocation share or latency benefit follows from the source change. Fresh leaf answers, complete owned projections and exports, numerical checks, timers and old-output ownership remain intact.

Proceed to Root-owned compilation, ordinary semantic qualification and complete paired measurement. No additional framework or implementation-shaped test is requested. No Cargo, compiler, formatter, test, runner, timing or network command was run for this review; only this review file was written.
