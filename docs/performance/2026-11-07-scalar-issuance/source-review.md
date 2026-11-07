# Scalar root issuance assessment

**Verdict: one bounded screen is defensible, using a scalar reference plus transition metadata in the existing RootOwner.** The current binary contains the allocation this would remove. This is a distinct ownership hypothesis, not evidence of a speedup or a requirement to keep searching indefinitely. Inlining the selection predecessor into every RootReference, or adding a transition directory, would make this substantially less attractive.

Read-only assessment, 2026-11-07. No compilation, tests, workloads, profiling, timing, network access, or repository changes were performed. Source was compared with immutable `c7508eec3ac44138305797691244ae9c180d7680`, whose retained runtime is `0e441c8ddeb6f85b9b0e8237303b5def32e178ae`.

The concrete cost is located. In the source-matched current diagnostic executable, `complete_root_change::h32150ceec103c5c0`, starting at `0x1009bd300`, requests 40 bytes through the malloc stub at `0x1009bd62c`; its return PC is offset `+816`. The old reference release includes an atomic instruction at `0x1009bd658`, followed by offset `+860`. The other issuance branch also requests 40 bytes. In `current-main-profile/16-1-center_move.sample.txt:1380–1384`, the apply-command subtree contains two inclusive observations at `+816`, beneath which malloc appears, and five at `+860`. These observations identify executed issuance work. They do not establish allocation counts, exclusive time, or a recoverable percentage of complete-operation latency.

The smallest useful representation is:

1. Keep RootReference opaque and Clone, containing its RootId and a private, process-unique serial. Keep `root()`, Debug, and strict issuance equality; do not replace identity with root, structure, MRU, or workspace-version equality.
2. Put `selection_from: Option<(previous_serial, NodeId)>` on the existing private RootOwner. A normal edit or changed presentation clears it; a selection issuance records exactly the immediately preceding serial and full generational Tabs key. No history collection or separate map is needed.
3. Replace the sole `follows_selection` consumer with a narrow query against the current workspace owner, at the same point in `WorkspaceIndex::after_selection`. Its existing membership, node-kind, and error ordering stay intact.
4. Use a private checked global AtomicU64 issuer. Workspace clones retain serials; independent builds and actual edits mint new ones. Discarded candidates consume serials without recycling them. No root-local counter, pointer-derived integer, epoch reset, key decoding, or wrapping increment is acceptable.

This can stay within `graph/authority.rs` and `workspace.rs`, plus focused existing test locations. The public reference need not become Copy, and command/target APIs need not change. The project already unconditionally uses AtomicU64 with relaxed checked `fetch_update` in `ids.rs:16–24`; atomic-width portability is therefore not a new requirement. Sharing the same implementation pattern is sufficient; root issuances should have their own private identity type and issuer.

Two semantic seams require deliberate treatment. First, `operation.rs:823–828` restores original issuance for a single Move whose final durable value is unchanged. `retain_original_issuance` currently restores both identity and its transition information by cloning one Arc (`authority.rs:430–435`). A scalar implementation must restore both owner fields together. General batches must retain their existing distinct behavior. Second, fresh issuance is currently infallible. A checked serial must fail closed on exhaustion, never reuse an identity. Keeping the ordinary Result/error precedence unchanged requires an explicit fail-stop exhaustion policy; the existing engine-domain issuer instead propagates its own exhaustion error. That policy difference must be recorded, not hidden behind an unchecked increment. If preserving infallibility even under serial exhaustion is required, this bounded design is unsuitable.

The selection move is justified by the actual consumer, not a general metadata abstraction. `WorkspaceIndex::after_selection` reads the current workspace's reference before calling `follows_selection` (`workspace.rs:195–206`); no consumer asks an arbitrary detached old reference to reconstruct its history. RootReference must still report its root after the workspace is gone. No-change restoration, candidate cloning, and one-step selection admission need focused review together.

Costs remain uncertain. Each reference becomes wider than one Arc pointer, affecting commands, targets, indexes, errors, and owner-map copies. The owner also gains an inline predecessor record. A shared global issuer adds contention on real edits, while removing allocation and reference-count operations. Exact aggregate sizes, code generation, and latency require measurement; the 40-byte allocation request alone proves none of them.

The meaningful witnesses are independent identical builds versus clones; discarded/staged edits and stale references; one-selection admission versus two selections or a different Tabs key; same-carrier no-op rehome; and no-change Move restoration of both identity and predecessor. Existing rollback, MRU-only history changes, and old-output lifetime checks remain mandatory. No new validation framework is warranted. If implementing this requires a second metadata repository or broad new error propagation, defer it rather than enlarge the experiment.

Source SHA256 bindings, all equal to the immutable source:

| Path under `crates/dockspace/src/` | SHA256 |
| --- | --- |
| `graph/authority.rs` | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |
| `workspace.rs` | `6cbcca776bfca60b1e767dc909121f06ae751110ea1f9fd265ea398da644aa36` |
| `operation.rs` | `7586ec06b29b77a794133e0c554d4fcca7a6faa16b8c550b86034ad394175862` |
| `ids.rs` | `28ab22cb6736b89376b9c3f1c99ed222aa54a172d16a51acaa8c457577b7f5db` |

Diagnostic executable SHA256: `35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590`. Evidence directory: `/tmp/dockspace-imgui-twofold-20261107-ea4161e/post-main-continuation/current-main-profile/`.
