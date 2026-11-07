# Independent review of the fresh-layout sampling diagnosis

**Findings first.** No blocking discrepancy was found in archive integrity, immutable source binding, the corrected diagnostic workload, or the reported sample counts. The evidence supports investigating projection continuation/storage costs alongside the other visible layout, Adapter, and allocation paths. It does not establish a complete-operation score, an exclusive stack-owner time share, allocation counts, or a predicted candidate speedup.

Two nonblocking interpretation notes should accompany this evidence:

- The static `source-binding.json` and its embedded copy in the successful receipt retain `performed: false`. That describes the original planning/binding record, not the final execution result. `receipt.json.runs`, successful process exits, and both completed stdout logs establish that the corrected workload ran. Consumers must not use the static field as the run-status authority.
- Report line 30 says ordinary counters are not bypassed in the non-test kernel. The runner does remove `DOCKSPACE_BENCH_NO_WORK_COUNTERS`, but the ordinary structural-work ledger is behind `#[cfg(test)]` and is absent from this normal-library build. The precise statement is that the bypass environment variable is unset; this is not an ordinary counter-enabled work qualification.

This is an independent read-only review of existing source, archives, receipts, logs, and executable bytes. No Cargo command, compilation, test, runner, archived script, sampler, or timing workload was executed. Only this report was written. No repository, cache, candidate, or fork was changed, and no agent was spawned.

**Evidence identity and completeness.** Reviewed `docs/performance/2026-11-07-layout-hotspot.md`, SHA256 `44c6b90364db365d9f41cbb782cf1d60ec870caa32efc2ac7710da309d9c6ded`, and its companion evidence directory. The source authority is immutable commit `a2365aa876c51a66a5c9c788bff9056afdbabacd`, not later live workspace contents. The retained diagnostic directory is `/tmp/dockspace-imgui-twofold-20261107-ea4161e/layout-hotspot/`.

| Artifact | Independently checked SHA256 |
| --- | --- |
| `evidence.tar.xz`, 27,760 bytes | `755f7406e0666170b2584e78d1a352f4590c255b9787911ac5f8aae3abd15ee3` |
| Internal `manifest.json` | `f3f79231e11618f2c98db75cec36aa8bc7aac4ff2c5ac9d8f0ab69f9816bcdc5` |
| `check.json` | `1b8e1a9cd067f113c1d053421ac5b0e2b429ae6ff8f20e3e788381a84f5bc352` |
| Final `receipt.json` | `728572a22b83d648a7bbe7bcac1ea2f7141e68a7e9adff2aa2db2f2ab6c63eeb` |
| `src/main.rs` | `a8532203edbe1580fa4abba707e296066fb9cec7916075ffceef7f2f20cd7ba0` |
| Diagnostic `core/src/lib.rs` | `e873d6c008ad43be8d8c0ee4980b17788f530f97f79914afcb1d7cf0fcddb7d7` |
| Corrected `Cargo.lock` | `d59b6ce3d83f84ce309cddeee81fa0c7663066be6e43f9a0bd3e38474ba0f6ae` |
| One-view complete sample report | `474f38a757be41923b146e0ddae1cbd6291f5152e945e559ad2d25dd6cabbc96` |
| Four-view complete sample report | `a84dbb9aefe980d197d6dc6d8865cc58ec0a785a374c689f37b54480b159c503` |
| Retained diagnostic executable | `1cba95baa365ee2fb6c56a9e31c6cca0279c6fd66e4dbb0983d839f19221c262` |

The archive has exactly 23 unique regular members: 22 inputs plus its manifest. All member paths are relative and contain no parent-directory escape. Every input's recorded byte length and SHA256 was recomputed, and the manifest has neither missing nor extra input entries. Twenty-one inputs equal their retained diagnostic-directory originals byte for byte; the remaining `immutable-base-commit.txt` equals the exact immutable Git commit output, including its newline. Both external `SHA256SUMS` entries match. These checks independently confirm the archive's round-trip claim.

The archive preserves the driver, runner as data, corrected package manifests/lock, the changed diagnostic lib file, both build receipts/logs, the initial lockfile, source-binding/evaluation records, and both complete sample/stdout/stderr/attachment-log sets. It records hashes and the immutable base for the other kernel sources rather than embedding a second full 262-file source tree.

**Immutable kernel and dependency binding.** The first and corrected receipts have the same 265 root source/configuration hashes: every one of the 262 Rust files under `crates/dockspace/src/`, plus root `Cargo.toml`, root `Cargo.lock`, and the core package manifest. Every recorded hash was checked against the corresponding immutable Git blob. The recorded Rust-file set exactly covers that subtree.

The temporary kernel also has exactly those 262 Rust paths. Of these, 261 equal the immutable source byte for byte. Its sole changed Rust file is `lib.rs`: exactly the private module declaration lines for `graph`, `ids`, and non-test `layout` become public module declarations. Existing test module declarations and all other bytes remain unchanged. The normal product facade is therefore not evidence of a newly public production API.

The diagnostic has its own small Cargo manifest, with default features empty and normal dependencies pinned to `slotmap = "=1.1.1"` and `thiserror = "=2.0.18"`. This manifest is an explicit configuration adaptation, not a claim that all Cargo files equal the original package. The corrected lock's eight registry packages match immutable root-lock versions, registry sources, and checksums:

`proc-macro2 1.0.106`, `quote 1.0.46`, `slotmap 1.1.1`, `syn 2.0.119`, `thiserror 2.0.18`, `thiserror-impl 2.0.18`, `unicode-ident 1.0.24`, and `version_check 0.9.5`.

The initial lock differs for six of these packages, including thiserror 2.0.21 and syn 3.0.9. Those failed-build versions are retained as history and were not silently described as the corrected sampled build. The first build receipt has return code 101 and an empty runs array; its log contains five E0603 privacy errors for the normal graph/identity/layout facade. The corrected build has return code zero and a finished optimized production-profile log. Its one warning concerns unused `RootProjection.ordinal/root` fields, not a build failure.

The outer manifest declares production thin LTO and one codegen unit. The recorded runner sets one Cargo job and clears the bypass variable. The successful receipt's executable SHA256 also matches the retained executable read as bytes during this review. The sample PIDs, process names, paths, attachment logs, and recorded run commands agree. These are source/build/run records and a retained binary identity; no new build reproducibility claim is made.

**Exact copied helpers and actual loop scope.** All five driver helpers match immutable `docking_cost.rs` byte for byte, including their function signatures and bodies. The original formal source SHA256 is `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3`.

| Verbatim helper | SHA256 of the complete copied function |
| --- | --- |
| `opposite` | `6328558761bbe4f0b875702f6d569c0406d7f3b385076bcbef67aefd280f2072` |
| `tree` | `5a1717ceefc6292959a0a6e91af136b37f767ef3330d59467af29b1b70347b4d` |
| `bounds` | `71d8e69c20dd95ee28b0ce155489c1a2857f8a580b7b018a153ad8022f727a44` |
| `collect_constraints` | `6ff0e77d5a3c604ebae8140b784a3f588680dfb2b467d419479302fa2711328f` |
| `project_all_roots` | `6aea9ab76f268cf6eb409732083c7b662d0d6533b41421c7e9c9ea3e94d15535` |

`src/main.rs:94–114` builds one validated 16-pane fixture before the loop, with one or four roots/surfaces and two items per Tabs. Its pre-loop witness checks root count and total projected nodes, `32 - views`, then is dropped. The repeated operation changes only bounds, alternating width 16,380/16,444 at height 16,380, and calls `black_box(project_all_roots(black_box(&fixture)))`.

`project_all_roots:50–92` creates fresh leaf constraints and a fresh per-root answer Vec on every call. `collect_constraints:32–48` actually walks the Workspace. The callback consumes the materialized answer iterator through `next_if`, checking the complete NodeId against the requested leaf. It is not replaced by a constant answer callback, even though the benchmark's leaf constraint values themselves are uniform. All roots are projected with the same original solver and rich owned layout representation. Each outer result is black-boxed and dropped at the statement boundary within the loop.

Each successful run therefore completes four million all-root Adapter iterations, plus its pre-loop witness; the four-view run is not merely four million individual root projections. There is no fresh fixture build per iteration, no mutation, request capture, export, complete-vector comparison, or ImGui condition. The diagnostic validates its intended execution shape, but does not newly qualify every public/raw geometry or error-order contract.

The disposal boundary matters. Immutable formal `docking_cost.rs:441–468` captures the final operation timestamp before returning owned projections in `Measured`. Their later disposal is outside that formal interval. The diagnostic repeatedly disposes those rich outputs inside its sampled loop. Report lines 11–16 accurately state this difference. Sampling proportions cannot be transferred to the formal layout phase or complete p50 by treating the workloads as identical.

**Sample counts and attribution.** Both attachment commands request four seconds with a one-millisecond interval. Recorded sampler and workload return codes are zero, each stdout ends in `completed`, and both stderr files are empty. The run PIDs agree with the report headers: 86936 for one view and 87022 for four views.

| Recorded quantity | One view | Four views | Evidence |
| --- | ---: | ---: | --- |
| Main-thread samples | 2,945 | 2,887 | Each report, line 24 |
| Selected `RawVec::grow_one` call-tree samples below `project_measurements` | 266 | 217 | Each report, line 34 |
| Collapsed top-of-stack `project_measurements` samples | 1,266 | 841 | Lines 231 / 220 |
| Collapsed top-of-stack `compose_constraints` samples | 278 | 250 | Lines 233 / 224 |
| Collapsed top-of-stack `collect_constraints` samples | 81 | 65 | Lines 237 / 228 |

The selected 266/217 node is a single inclusive call-tree branch, with realloc/allocator descendants. It is not a total of all RawVec calls, all allocations, or all samples involving allocation. Both reports retain other RawVec growth/reserve paths, the fresh answer gather, direct allocation, disposal, and free paths. The reports' collapsed summaries and recursive-count summaries use different aggregation rules; neither should be added to an inclusive ancestor to construct a causal share.

The immutable projection executor contains a growing continuation Vec in `layout.rs:956–994`, including pushes of split continuation states. That source fact and the observed growth branch make continuation storage/control-flow a reasonable bounded experiment. Optimized/inlined stack symbols do not, by themselves, prove the exact source owner or isolate its exclusive cost. No source-line/type mapping in this evidence turns the generic RawVec symbol into an allocation census for that owner.

The documentation respects that boundary: it identifies the selected branch, retains other stacks, limits the inference to experiments, and explicitly rejects exclusive formal time shares, allocation counts, residency conclusions, or latency predictions. No elapsed receipt interval was converted into a benchmark score in this review. This evidence leaves the complete-operation target and any future candidate's semantic/performance qualification to their separate unchanged-kernel checks.
