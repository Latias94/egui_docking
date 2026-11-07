# Final lean-source and formatting review

No source blocker found. The lean cleanup removes exactly the three redundant lines identified in the original independent review. The applied source is exactly the lean tree plus the supplied formatting patch; no algorithmic delta was found.

The immutable parent remains `9ddb63f425ca8316bdf483d6297e923c35a16ac3`.

| Artifact | SHA-256 |
| --- | --- |
| Lean full `candidate.patch` | `5323fda66a4bed2557d4e67b08b71019eac1eef16a55d9585d70d5bb7aba0b55` |
| Lean `cleanup.patch` | `0d1ac05be7c5c6b3872837aeff4e25855a58126de51822b6a7ca4495b8117238` |
| Lean `manifest.json` | `0a0f52676d287db468d502c0a248d2e1613de478c02941a0eafddbd6e6298e95` |
| Lean `READY.json` | `485f8e97f9d99746ae8cf8daae7ce9342c24b0d6b85f36ebcb118b7ecc3f9e3f` |
| `native-motion-verified/formatting.patch` | `78ea72d57e643df71683c27503653f167c81891b72b0f7f6709a3cabfc1e85f6` |

Independent comparison of all four original/lean source pairs confirms **+0/-3**: removal of the latest `MotionReceipt.token` field, its assignment and its duplicate baseline filter. Every other source byte, including the complete regression section, remains unchanged. The real token lookup and viewport filter, context/window/create-attempt reset, retained lifetime anchor and held-state baseline requirement remain intact. Manifest file/artifact digests, READY binding and declared source-set digests match.

The formatting patch changes only import ordering, line wrapping and compact formatting of the existing two-condition filter. It changes no expression, event order, deadline, input, assertion or branch behavior. Independent in-memory application to the lean tree reconstructs the current three formatted files exactly; the fourth file, `assertions.rs`, remains byte-identical to the lean tree.

Formatted source SHA-256 values:

- `app.rs`: `d142ac0f8f9d95f61dfa6560c1e5d5870a93d6e79e6910ddbba42461b8f0a52c`
- `driver/scenario.rs`: `11006f9f9bc5eaa0057cec76238c32b6c66a67ed725d077ee611050a94f62314`
- `motion_receipt.rs`: `25f1280ade04d8b3fa409f75cf257b568afade94676b5a23c7c740f22591acfe`
- Unchanged `assertions.rs`: `981dfae5b1f8e3f6bae4ddeb45ab84e5fb92c473e4ae8aff9395414da6505733`

This review adds no execution claim. Root owns the running verification. macOS Context/physical-binary unit tests do not execute the Linux X11 driver or prove physical drag-back success. Neither the cleanup nor formatting resolves the historical native event ordering by itself.

Only this report was written. No Cargo, compiler, formatter, test, timing, network or candidate/repository/fork/cache mutation was performed.
