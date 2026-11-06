# Pointer retirement evidence review by the fix author

Mechanical evidence checks found no blocker in `docs/performance/2026-11-07-pointer-retirement.md` and its committed archive. The local execution claims, exact source identities, patch history and stated physical-CI limit agree with the preserved evidence. I am the fix author (`/root/root_frontier_placement`), so this is not an independent source or authority-boundary review. That review belongs to `/root/mru_rebase_review`, whose original report and final test supplement are preserved separately. This report checks evidence consistency; it does not rerun qualification or establish the exact rejected physical callback.

The immutable candidate is `1c7169618e90834f1f9b007a59254c46aee8ed35` (`fix(native): retain exact queued pointer facts through retirement`). The control is `c98569d0b1424cb8ee0da724b1aba75b578bad78`, exactly as recorded by `actual-control/commit.txt`, `checks.json` and `archive-check.json`. The control is a source comparison point, not an assertion that it is the candidate's immediate parent; that parent is `27caca6cfef8937110e7c14b40ff413432d621dd`. All reviewed documentation and evidence-directory bytes match the immutable candidate commit. Later performance work is not substituted for this qualification identity.

| Bound artifact | SHA-256 |
| --- | --- |
| Performance document | `49030ef4487df95ddd561361776ca679456fc90809436c47d33744c5a8380802` |
| `evidence.tar.xz` | `0b687f1dc0937e6c1a90118684b2c9ef26a135914e9db8abf79ea4d188bfd659` |
| Archive's `manifest.json` | `8337c1529942e3df8fe0e758be71c65768bcf0520bb8f5b968e185c29b330e75` |
| `checks.json` | `a94e8d4f795639a7b9702667dcfa5de6097a6b6762618967787687c990ea48ff` |
| Original independent admission review | `580cd13eadda6c68910f93fa7437eabbdc314bd9142775897d6026cfd30fc120` |
| Final independent test supplement | `05b939a75fedee81fe72130dbbe8ec5d817572d9e320598d81b3fdd36397d1c9` |

The archive contains exactly 60 manifest-listed inputs plus the manifest itself: 61 unique regular members, without duplicate names, absolute paths or parent traversal. Every input's full SHA-256 and byte count matches its manifest entry. All five outer SHA256SUMS entries match. The 52 investigation/remote-log members also match their still-present original temporary files byte for byte. The other inputs bind the three immutable control files, three immutable candidate files, control commit text and complete final diff. Archive data was read in memory; no member or script was extracted or executed.

The complete source pairs match both immutable commits, the reviewed candidate tree and the successful qualification's three named fix hashes:

| Source | Control SHA-256 | Final SHA-256 |
| --- | --- | --- |
| `crates/dockspace/src/runtime/native/pointer.rs` | `4f830aa26aea09c5d0cf63c1a81309e7f05a7bbe9ce4d0a22b3a9b6697ebc03e` | `852e72b524d6fdbf532b8e05be052b2c3b0ad56cfcbff91d768d28ac6eb946d5` |
| `crates/dockspace/src/runtime/native/session.rs` | `3d1ee57951473d89afa63a7c7b2b62d0f9ded01ba3c192b8c0b544b9caf795e5` | `5182741a8c7dc9eb005ba44e555cf0b929252f1b8161150f341f1388e926845f` |
| `crates/egui_dockspace_native/src/coordinator/tests/retirement.rs` | `614b1875cbae55a96a3e6074639cfa98ba51e8e28d53338248aadd6887b6b493` | `b81b94195de169adb1e976f076bc2341c16165b8386766cbeafc764598364137` |

All 321 unique source hashes in the preserved focused/integration execution receipts match immutable `1c71696`, beyond those three fix files. The current three fix paths also matched the final hashes when read; the verdict remains bound to the immutable commit rather than mutable workspace state. This is consistent with the documented bounded change: ordinary journal facts use exact recognized lifetimes, the recovery roster retains its separate current `contains_binding` guard, and reducer/fresh-capability authority checks are unchanged. The independent admission review, not this author's evidence check, supplies the source-boundary judgment.

Three different patches have different valid bases. Applying each in memory against its recorded complete base, with exact context matching, reconstructed these identities:

| Patch | SHA-256 | Exact role |
| --- | --- | --- |
| `investigation/causal-red.patch` | `14803257fbb036253abdba2cf2d7e13f64f20dbdbb7137cfaee2846d965dbcd3` | Original control test to the retained-callback RED; production files unchanged |
| `investigation/fix-candidate/fix.patch` | `7a0776a2c51201cbdadc768453e8a02bfccee463bcbc517c090c7567c4799c4a` | Already-RED base to the reviewed production fix, documentation and added boundary assertions |
| `actual-candidate/runtime-and-test.patch` | `c9a35bdab2e8c4413896129129297cf88c2e1a562bdbe7277169a557d6f80728` | Immutable control to the complete final three-file result, including the original RED test delta |

Both control-to-final routes produce exactly the archived final source bytes. The reviewed fix patch must not be presented as the entire control-to-candidate diff: its test base already contains RED (`4bb735753022016ee52ea505dd8e988846e574be1748db3b38823d5ba43dd17b`). The archive preserves this distinction correctly.

The RED is an actual compiled behavior failure. `investigation/executed/causal-red.json` records exit code 100. Its matching log contains successful completion of the test-profile build, one executed test, zero passes and one failure. The panic points to `coordinator/tests/retirement.rs:822:14` at `expect("the retained pre-suppression callback must drain without InvalidPointerFacts")`, with the exact `NativeRuntimeError -> DockspaceRuntimeError -> Native(InvalidPointerFacts)` chain. The reconstructed RED source contains that expectation at the recorded location. It is not a scope/type error, a build failure or a different earlier assertion.

The scope-error draft is historical diagnostic evidence, never a passing candidate. Its patch hash is `3da27ce04d8bd8fb1310f4577cc37ba9d5dcbc70330576ef98d85c520e172690`; its erroneous test hash is `291e269399bb221f7997bbea091fa95ec3d417e8f1048745f18f94d1e2e0c4c4`. The archived draft explicitly records `production_fix_applied: false`. The subsequent correction receipt preserves the old patch, records the original repository files still matching base, and relocates exactly the same fourteen lines into the intended test. Removing that block from the erroneous and corrected test yields identical bytes. The preceding scroll test is byte-identical to the RED base. All GREEN execution receipts bind the corrected `b81b...` test, and none identifies the erroneous draft as executed or passing. This supports the document's before-application correction history and keeps it distinct from the earlier genuine RED.

The eleven successful check entries in committed checks.json equal the four focused and seven remainder receipts exactly, and every return code is zero. Their log summaries agree with the document:

| Check | Preserved result and scope |
| --- | --- |
| fmt | Successful all-workspace fork-manifest check; empty log and exit zero |
| causal GREEN | The same registered retirement test passes; 545 other tests filtered out |
| core boundaries | 340 passed; 1,732 filtered/skipped by the focused selection |
| native library | 545 passed; one existing case skipped |
| workspace | 2,245 passed; three existing probes skipped |
| Clippy | Successful correctness/suspicious gate; not a claim of all warnings denied |
| documentation | Successful core/adapter documentation; preserved driver sets `RUSTDOCFLAGS=-D warnings` |
| doctests | 9 core + 0 host-conformance + 1 adapter passed: ten total |
| fork workspace | 593 passed; one existing case skipped |
| native without debug assertions | 545 passed, one skipped; command explicitly sets `profile.test.package.egui_dockspace_native.debug-assertions=false` |
| dependency boundaries | Preserved log reports normal/build boundaries passed for all features and targets |

The source-before/after receipts report unchanged inputs and normal work counters enabled; the preserved remainder driver removes `DOCKSPACE_BENCH_NO_WORK_COUNTERS`. The no-debug run changes only the native package's test-profile debug assertions. It is not a whole-workspace release qualification. These overlapping suites are separate checks, not a count of unique tests to be added together. No new end-to-end saved-local-sample plus successor geometry plus active release scenario is claimed; the document preserves that coverage limit and points to the independently reviewed registered components.

Physical attribution is appropriately limited. The preserved old remote snapshot is completed/failure for run `42297763668`. Its physical log records the guide-control path, verified Center merge, physical release, drag-route restore and then fatal `InvalidPointerFacts` during `await-retirement-quiescence`. It does not identify the rejected callback or exact delivery/hover/capture owner. The local RED proves the retained-lifetime/admission mismatch in its actual FIFO fixture; it does not prove the old X11 run's exact callback chronology.

Root reports pushed run `42308166838` for the new commit is still in progress. This review made no network request and preserves that run as pending, not a success or a newly fetched live status. The archive contains the old failure, not an outcome for that new run. Local macOS unit qualification cannot establish physical X11 repair. Fresh pushed-head physical smoke and all remote CI results remain required; neither the document nor this report declares CI repaired.

Only `/tmp/pointer-retirement-evidence-review.md` was written. No Cargo, compiler, tests, timing, runner or archived packaging/verification script was executed. No repository, fork, cache, source candidate or existing evidence was changed; no sub-agent or network was used. Read-only Git object access and in-memory archive/hash/patch comparisons supplied the mechanical checks above.
