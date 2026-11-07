# Measurement child-row iterator review

Verdict: PASS. No source correctness, error-priority or exact-base blocker found for patch `3f826644008677c59c44917c4f38dda5542ccd3ccdca286d379c572a3a34322e`.

This is a source-only review of the frozen three-file patch, its exact base/tree/contract snapshots, and the relevant local call surface. No Cargo, Rust compiler, product tests, benchmark, timing, formatting, Git write, repository edit or candidate edit was performed. Only this report was written. Compile/behavior qualification and any measured benefit remain root's work.

## Exact patch and base

The 6,941-byte patch reconstructs exactly from its saved three before/after files, with +26/-40 lines. Per-file lengths and hashes, the sorted three-path base/candidate set hashes, all six unchanged contract snapshots, composition patch identities and READY/manifest/README bindings verify. No unlisted source hunk exists.

The base `layout.rs` equals the reviewed current slab output; topology/reference equal the frozen single NodeResult inputs. The original continuation, shared RootStructure payloads, single NodeResult ownership and slab storage are retained. There is no compact-frame, weight-fusion or solver-aggregate change. The current RootStructure successful-build hook and ordinary work CSV remain in the unchanged context snapshots. Application must preserve those snapshots' current counterparts and use exact hunks after the matching slab, rather than overwrite files with older complete sources.

| Path relative to `crates/dockspace/src/` | Exact base SHA-256 | Candidate SHA-256 |
| --- | --- | --- |
| `layout.rs` | `e4597343ab8b0f8d69f94bebf7e23e190905dace9e77b942b2187c2ef555d26c` | `026384059d20282efcf773ad0daebc2722b0b2feb493933310ea9418e2fd6372` |
| `layout/reference.rs` | `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a` | `ea4afaf3fc82be4e3f27f8c00aa558d5aed0ff37ff15a184d3db3dc49f7e7b27` |
| `layout/topology.rs` | `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56` | `89e94dc277cbfe3d4734ddd16f25fffee4c735b5144f75fd3e4b119006ece9c9` |

Base set: `3eb2580e07bb0c3d670be826083b403f61c4dacaaefa769f0ff89784e1d70984`.
Candidate set: `9a0c41e3751877468dc92bcf90a9407bb89d1d16e87157f9e2de887da6ad68e1`.

## Actual iterator and borrow behavior

The complete affected call surface is bounded: three calls to compose_constraints (production, checked raw, reference), one call to compose_minimum, and three to compose_maximum (normal composition and both production/reference minimum-only completion). The helpers remain private. All passed iterators are standard slice Iter/map combinations; no arbitrary public callback or stateful provider is added.

Production `topology.rs:160–171` iterates the exact immutable child-row slice and copies Constraints from the already completed measurement rows. Map's Clone copies its slice cursor and closure's shared reference. It does not copy or clone the Vec, run the mapping closure, allocate, call read_leaf, or evaluate Constraints constructors. Iterator consumption does reread and copy the existing row's Constraints for each numerical pass; that extra indexed-read behavior is real and may affect performance. It is not repeated fresh measurement or repeated solving.

The minimum-only completion at `layout.rs:608–622` reads child constraints before taking the parent mutable reference. Production measurement constructs the entire RHS, including successful composition and central propagation, before assigning values[row]. No iterator or reference escapes in the returned owned Constraints. Therefore parent writes cannot interleave with a clone's reads or invalidate a Vec borrow. The current postorder and valid immutable row ranges provide the same completed child values that were previously copied into scratch.

Every leaf still invokes read_leaf once at its original postorder point. Fresh initialization, all measurement rows and current root central metadata remain per-call inputs. The removal of the gather buffer neither adds a callback invocation nor omits an actual leaf.

## Numerical order and error precedence

The minimum and maximum arithmetic bodies are byte-identical to the base, independently recomputed as 561 / 581 bytes with hashes `4e8215a88b21f8fe237917998d657a31c11be92b04f380c0e94c13e4bfcd2749` / `5e586e971532c121cea91f36250b4fa93edda3bd9508979419e3615a9a7e463c`. The full public/entry prefix, fresh output/override/measurement region and complete splitter/DFS/axis-solver suffix are also byte-identical.

For each pass, the new map yields exactly the original child sequence's Constraints, without arithmetic in the row lookup or copy. Width and height retain separate passes and the same evaluation order. Horizontal sums consume child widths followed by the splitter extent; vertical sums consume child heights followed by that extent. The other dimension uses the same max/min folds, with initial values 0.0 / infinity. LogicalSize construction, checked_sum quantity strings and Constraints::new remain in their original positions.

I also read the installed pinned Rust 1.95.0 standard-library source: float Sum uses `iter.fold(-0.0, |a, b| a + b)` (`core/src/iter/traits/accum.rs:156–165`). Map::fold forwards the supplied accumulator through the underlying iterator; Chain::fold processes the first iterator before the trailing iterator. Thus the added map layer does not change reduction grouping, initial signed zero or the trailing splitter addition. No handwritten sum, reassociation, cached aggregate or fused width/height reduction is introduced.

compose_constraints still checks zero cardinality before computing splitter extent, minimum before maximum, and validates the final Constraints afterward. Minimum-only composition still skips the maximum pass until bounds are known. Its subsequent maximum completion retains the same child postorder, fresh leaf maximum and parent update.

Production previously gathered valid rows before splitter validation; it now creates a lazy iterator before validation. Those row reads are infallible, side-effect-free reads over a graph-owned valid directory. Moving them later therefore changes no reachable domain diagnostic. Crucially, the checked raw path retains its fallible child gather before composition, including all missing-row, incomplete-row, active-cycle and cardinality checks. A missing raw child still wins before a parent's splitter/composition error. Reversing its one argument adaptation reproduces the entire original raw compiler.

contains_central is still an ordered short-circuit any evaluated only after successful composition; the final root reachability check remains after the complete measurement traversal. The central error is not moved ahead of a leaf, aggregate or geometry error. Public override-before-leaf and minimum-measurement-before-override boundaries are in unchanged source. Solver validation, arithmetic, errors and both original assigned-size sums remain untouched.

## Oracle boundary and old outputs

Reference changes are exactly two call argument adapters. Replacing those two iterators with the previous slice arguments reproduces the entire prior reference file byte-for-byte. Its traversal, fallible child gathers, raw diagnostics, central propagation, independent owned output types, capture and separate axis solver remain unchanged. Reference deliberately continues gathering its children while production directly reads rows, so output/error comparison can expose wrong row joins, ordering, omitted rows or stale values.

The reference already shared compose_constraints/compose_maximum before this patch. It is therefore an independent traversal/output oracle, not an independent arithmetic implementation for these helpers. Changing their signatures does not erase a previously independent arithmetic oracle, and the README states this limit correctly. A shared helper bug could affect both sides; the arithmetic-body equality and direct reduction-order audit above are the evidence for this shared boundary. The reference acquires no production child-row reader, slab helper or new algorithm.

RootStructure, complete public output, slab ownership/Clone, previous output lifetimes and the current benchmark source are unchanged. Each measurement still owns a fresh Vec and each projection owns fresh result values. Failed composition or later geometry construction exposes no partial output and cannot mutate an older output. Existing full-value cold/minimum oracle coverage, fresh-reader/metrics behavior, minimum/override priority, raw completed-subtree/cycle/missing-node order, sparse generation, cloned old output, late geometry failure and registered 10k projection/drop tests remain unmodified. No extra mirrored test is needed for the container substitution; this review does not claim those tests have run on the candidate.

## Cost and remaining gate

The source cost statement is accurate: production measurement removes its one reusable child-gather Vec and explicit per-edge SubtreeMeasurement materialization. Minimum-only maximum completion removes its separate gather Vec and copies. Raw/reference copies remain. The same numerical passes, complete outputs, actual leaf queries and timer boundaries remain. Direct indexed rereads and bounds checks may offset allocation/copy savings; no speedup or allocation measurement follows from this source review.

The candidate is suitable for root's ordinary serial compilation, retained behavior gates and separate controlled comparison against the exact qualified slab base. No source correction or new regression framework is requested.

## Frozen artifact binding

| Artifact | SHA-256 |
| --- | --- |
| `measurement-child-rows.patch` | `3f826644008677c59c44917c4f38dda5542ccd3ccdca286d379c572a3a34322e` |
| `base-manifest.json` | `6342d75e456e6c945c854eb8f959b3c68c0db48c2635cd207d2fd43ddef942ee` |
| `manifest.json` | `776d4c9182843d6f8df3ae66c580eb1866f8d2101785e2dfb5cded0ce655e99c` |
| `scope-checks.json` | `618381620c52c48f420e4bcef6d5038c95eedadef7af033e3d4801c16fe273c8` |
| `README.md` | `8308b3e9d28f58183029de85d4490fc800398dec3f4889ce78b14f1d7a1ad32d` |
| `READY.json` | `a9f444ebc8b4315bd84df207f8c0f583d088b2b60794399b9006c1497220bb1d` |

The pinned standard-library sources read for the reduction argument are local Rust 1.95.0 `traits/accum.rs` (`ded19e8fda12eb066c97d237afcdfe3c44d058494e3e51f4e291807bcbba08a0`), `adapters/map.rs` (`2a9cb86b5110b292f271ec1cd8904a9bad27f8d553af0141837f41e753fe83d3`) and `adapters/chain.rs` (`64e46d2857d1df7bf3e4028e9183f83c237e51d88e3d31c740d43e976b8d0231`). They were read, not compiled or executed.
