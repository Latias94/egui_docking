# Final ergonomics report and evidence check

No blocking report/evidence discrepancy was found. The local verification counts, actual-window journey, screenshots, and environment description support the report's stated scope. This check does not establish remote CI success or a new docking-performance result.

Reviewed: `docs/audits/2026-11-08-interface-and-interaction-ergonomics.md` and its `2026-11-08-ergonomics/` artifacts. This is a mechanical evidence and report review, not another independent source review: this reviewer contributed the construction, owned-composition, and native-configuration changes. No build, test, GUI, network request, or candidate script was run. Only this report was written; container access was limited to read-only metadata and hashing the existing binary.

## Verification receipts

All 12 entries in `verification.json` have return code zero. The compressed logs independently contain:

| Gate | Independently counted result |
| --- | --- |
| Workspace | 2,252 PASS records; summary preserves 3 skips |
| Official consumer/demo | 27 PASS records; zero skips |
| Product consumer | 119 PASS records; zero skips |
| Native workspace | 599 PASS records; summary preserves 1 skip |
| Doctests | 9 + 0 + 1 + 1 = 11 passed, including PaneResponse |
| Three Clippy gates | Successful Finished records; no error |
| Linux production build | Successful optimized production build |

The three formatting commands also have successful receipts. Existing ignored-test attributes are unchanged from base `2697b04`; no new skip was introduced. The three Clippy commands specifically enforce `clippy::correctness` and `clippy::suspicious` while allowing other warnings, which the report accurately names. The native test log retains ordinary dead-code warnings; these are not represented as a warning-free build.

The final official log includes the actual close/restore and all-groups-closed/Restore-button tests, as well as the continuous journey. The native log includes all three new constructor tests. These are successful CPU semantic tests, not physical-input or GPU evidence.

## Journey and source binding

`frames.csv` has exactly 384 data rows: painted_idle, hover, text_edit, scroll, splitter_drag, and tab_drag each have 64 samples in order, numbered 0–63. Every row has positive vertex and index counts. No phase or sample is missing or duplicated.

The actual-journey receipt records success and an elapsed interval of 47.385337 seconds, correctly rounded to 47.39 seconds. Raw stderr confirms completion of text, scroll, resize, exact tab merge, eight items, and 384 samples, with `startup_dropped_outputs=0`.

All 52 recorded source SHA-256 values match the current files. That source list exactly covers every changed Rust path relative to `2697b04ca9733af173863419d657bdac5d67de5d`. The archived CSV, journey/capture receipts, capture script/output, and six screenshots also match their local originals byte-for-byte.

## Environment and visual scope

The copied environment differs from the earlier host environment only in the binary and CSV hashes. Both updates are correct. Read-only inspection confirms the same image ID, one-CPU limit and 4 GiB container limit. Hashing its existing `/native-target/production/official-caller-owned-demo` independently matches the new recorded binary hash.

The retained runtime environment and observed adapter line agree with current stderr: Linux/Xvfb X11, scale 1, `LP_NUM_THREADS=1`, and llvmpipe CPU rendering through WGPU's GL backend. This is not an Apple M4 or hardware-GPU measurement. The report correctly treats the added toolbar as a changed workload and makes no before/after performance claim.

I inspected all six PNGs. The baseline shows clipped Inspector title glyphs; the after image shows the toolbar and readable title. Edited and restored both contain `~ERGONOMICS~`; closed shows Editor removed and Preview active. The splitter image visibly includes the purpose and keyboard-adjustment tooltip. The retained capture script uses X11 pointer/key injection against one application window and its actual Restore button, while preserving the property warnings in stderr. This supports the finite same-window visual journey, not native cross-window authority, physical hardware input, or general platform certification. The timed journey and subsequent untimed screenshot capture remain correctly distinguished.

## Evidence fingerprints

| File | SHA-256 |
| --- | --- |
| `verification.json` | `b7c453abde446b86c80914ca5bc67a89c0601806afd6bc39757a872ff9c1b8c0` |
| `actual-journey.json` | `bb809befa3d4a6a3df8e5f9aedcfe8d3615334d5c45e833f4239b8839437cc45` |
| `environment.json` | `2acebfb0d33aa8b63e60536d813bbb67dc134827bd348fdf139fd72681b3511a` |
| `frames.csv` | `19a98516abac2d1d18becc7ca380cf7ad34cb92e7e1814ced29277ce8c4239d3` |
| `capture-ui.json` | `2c89e9f4465e77a91492f719b1542a1b434869618c15624ce16b04c5ae64cf49` |
| Existing Linux production binary | `7c68e20d609c04a34f24db390d19330a86cd61e1dab1c0a68e89c3738d88e553` |

No stale environment value requiring a report correction was found. Publication and any final-head CI remain separate work for Root; this evidence check does not declare them complete.
