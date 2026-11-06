# Independent supplemental review: placement-sharing test policy

**No blocking source issue in this exact test-only correction.** The original test used a Native rehome with the default policy, which disables Native presentation. My earlier source review missed that fixture permission and its test assessment was incomplete. This supplement corrects that assessment; the original review and frozen candidate identities remain preserved.

Completed at `2026-11-06T22:24:00.208620+00:00`. Only this supplemental report was written. No Cargo, compilation, test, benchmark, timer or repository/candidate edit was performed by this reviewer.

## Exact correction

- Original frozen authority source: SHA-256 `cae68a84471cc9fa64f74c7345c3566c8b7a3e8b84f870060b55ab2f4e44164a`.
- Corrected authority source: SHA-256 `2c618031ea69e0dd3b4e6ccd16f7f34bbc4c52b1f733abaa515bf356ad4422d4`, matching `placement-sharing-test-policy.json`.
- Test-only patch: `placement-sharing-test-policy.patch`, SHA-256 `4279fc237f01773de9c61afee36c1e88bacc1e42dc28b42f51c1428bad45490a`.
- Supplemental receipt: `placement-sharing-test-policy.json`, SHA-256 `f5b45bc7a69a2c9b57c143773c6bf64aad8f01d878b272146755c5036d2e8425`.

Independent diff reconstruction reproduces that patch byte for byte. The corrected full file equals the original frozen candidate after exactly two substitutions: import `DockPolicy`/`PolicyRevision` instead of `DockPolicySnapshot`, and construct the fixture policy with `DockPolicy::default()`, `set_allow_native_surfaces(true)`, then `snapshot(PolicyRevision::default())`.

The runtime prefix before the test module is byte-identical, SHA-256 `cb7626c39c85ff61b52cd56a0e0672215b2f8d274f20d21fc310890300909990`. Every command and assertion remains identical, including changed reports, placement Arc sharing/detachment, original/changed reference validity, original MRU, actual changed parent link, prepared-candidate drop and unchanged live state. No runtime authorization check, production default, policy outcome or unsupported-operation bypass changed.

## Why this permission is correct

`DockPolicyRules::default` permits Tiled and Contained presentation but omits Native (`policy.rs:677-692`). `DockPolicySnapshot::default` snapshots that policy (`:1790`). `set_allow_native_surfaces(true)` uses the existing policy API to enable both NativeSurface operation and Native presentation (`:844`, `:855-872`). It does not assert provider/platform capabilities or dispatch a native host operation. The test remains a renderer-neutral checked graph transaction, now with the explicit application policy needed for its supported `NewSurface` rehome witness.

The preserved initial receipt under `placement-sharing-verified/initial-policy-fixture/` binds the failing original hash and records formatting success, targeted exit 100, and 427 passes plus one failure. The failure is `Policy(PresentationModeDisabled { mode: Native })` at the transaction unwrap, before the rehome sharing assertion. It is not evidence that placement COW or rollback failed.

The existing corrected receipt `checks-fmt,targeted.json` binds the corrected source hash and reports formatting/targeted exits 0. The corresponding archived-on-disk log summary read by this review is:

```text
     Summary [   4.419s] 428 tests run: 428 passed, 1644 skipped
```

These are existing root-owned execution records; I did not rerun them. They establish the corrected focused result only, not completion of broader qualification or performance work.

The original frozen patch `cfe9f0cb896c2db6aaa23de17d40328ae279a69038a6869698275fa84ec0e3c5`, original manifest `f1148d3bc7a7ed9396121a6520692a12abc72cd1740d0614fe3845d7a4e1e724`, and original review `e33a62cb137a9a54951ae378de25ff4e02e7dcaa5366940d347a244e85bd5122` remain unchanged. This corrected test source is a separately identified revision; do not relabel the old candidate hash as the corrected one.

Supplement complete; STOP.
