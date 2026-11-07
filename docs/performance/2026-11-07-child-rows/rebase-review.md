# Child-row candidate rebase review

This is a read-only source-composition review. No Cargo, compiler, test, timing, runner, or archived script was executed. No repository, fork, cache, or candidate source was modified.

## Conclusion

No rebase or source-composition blocker was found. Applying the original child-row hunks to the retained three-vector control yields exactly the three candidate source hashes recorded in the application receipt. The runtime diff preserves every original added and deleted line, in the same path order, at +26/-40. It does not restore the rejected extent slab.

This conclusion concerns exact composition and the small rebase boundary. It does not qualify the candidate, replace semantic checks, or establish a performance benefit.

## Exact inputs

- Immutable retained control: `e3c517195ddce2c7d1a6dbfd4328f152978968ec`.
- Head recorded at application: `16e4f9440ad225088f4b1eab2d2677c741705e31`.
- Application receipt: `/tmp/dockspace-imgui-twofold-20261107-ea4161e/child-rows-application.json`, SHA-256 `86247527756de10c71c160efeeb3f90bb9b70ce4e6806bfdf524c97ba9e080ed`.
- Original patch: `/tmp/measurement-child-rows/measurement-child-rows.patch`, SHA-256 `3f826644008677c59c44917c4f38dda5542ccd3ccdca286d379c572a3a34322e`.
- Runtime diff against the immutable control, limited to the three touched paths: SHA-256 `9909e84c05fc98d4e59732700fc829dd7902490b6421aa10e3ed1a949877d90b`.

The following paths are relative to `crates/dockspace/src/`.

| Source | Actual retained base SHA-256 | Original candidate base SHA-256 | Applied candidate SHA-256 |
| --- | --- | --- | --- |
| `layout.rs` | `325f56f16cacbde46cce531461c8830417a6aa8ff4af06f6c87145a7528963df` | `e4597343ab8b0f8d69f94bebf7e23e190905dace9e77b942b2187c2ef555d26c` | `9ba215565d486e56d7c5435e123691948fece5a25ff5479bb245f793c4ab989f` |
| `layout/reference.rs` | `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a` | `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a` | `ea4afaf3fc82be4e3f27f8c00aa558d5aed0ff37ff15a184d3db3dc49f7e7b27` |
| `layout/topology.rs` | `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56` | `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56` | `89e94dc277cbfe3d4734ddd16f25fffee4c735b5144f75fd3e4b119006ece9c9` |

## Mechanical checks and rebase boundary

All nine old hunks of the original patch matched uniquely when applied in memory to the three immutable control files. The reconstructed outputs matched the application receipt and the three live source files read during this review. A separate `git diff` against the immutable control matched the recorded runtime diff hash; extracting the complete added/deleted line bodies from that diff and the original patch produced identical sequences.

Only `layout.rs` has a different original base. Its original-base versus retained-base differences are the rejected slab's three prepare/solver/advance access changes around lines 999, 1034, and 1120. The child-row hunks are around lines 605, 798, 821, and 850, so none overlaps those slab accesses. The topology and reference original bases are already byte-identical to the retained control.

`layout/projection.rs` read during this review is byte-identical to the immutable control, SHA-256 `d86c008dd78c5f1ed805de76b71d102a29dd3dab4e6ee37ca8f3cb74839edfb0`. It contains neither `child_extent_data` nor `fn child_extent_slices`; the applied `layout.rs` contains no `child_extent_slices` call. Together with the exact three-path diff, this rules out a hidden slab restoration in this application.

The preserved hunks remove temporary child-value collection from the measurement and delayed-maximum paths, pass cloneable exact-size iterators of `Constraints` to the existing composition helpers, retain the raw compiler's gathered values, and adapt the two reference call sites. The sums and folds keep the original child order. This rebase adds no changes to those original bodies. Existing source-review limits and the need for Root-owned verification still apply.

## Historical scope

The receipt, immutable Git sources, patch hash, and reconstructed output hashes bind the application independently of later repository edits. The live comparisons above describe the files when read; this report does not treat subsequent WIP as the measured or restored slab source.
