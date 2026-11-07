# Source-nonempty shortcut: contract review

No source correctness blocker found. The shortcut is valid for both private callers and is suitable for Root's ordinary qualification. This is a source review, not a test or performance result.

Reviewed `/tmp/dockspace-imgui-twofold-20261107-ea4161e/post-main-continuation/source-nonempty-source/`:

| Artifact | SHA-256 |
| --- | --- |
| Base `crates/dockspace/src/operation.rs` | `7586ec06b29b77a794133e0c554d4fcca7a6faa16b8c550b86034ad394175862` |
| Candidate `operation.rs` | `bf81e49acdf0678cbb25cdf1f769fa9e465a45a9034dd349342398f839eee349` |
| `runtime.patch` | `a67c0b88a9f0a9e6c77ae8c5e1084c72f3ac958fd454211f21121d65cb95a2db` |

The complete-file diff contains four hunks, `+21/-10` lines. The live file matched the candidate exactly at review time. No tests were added.

## Why the shortcut preserves behavior

`remove_item` performs the same node/type/item checks, item removal, MRU lookup/removal and selected-item update before returning `!items.is_empty()` (`3550–3587`). Existing failures still return before a boolean is produced. The boolean is a current local fact, not stored authority or a replacement for source validation.

For Item detachment, `move_payload` still checks source, target, semantics and policy before mutation (`1203–1220,1761–1769`); Item membership includes exact reference and item validation (`2769–2794`). Root-content creation also validates its payload before `detach_root_content`. No operation between Item removal and cleanup disconnects or reparents that source tabs node. Consequently, remaining items in that reachable node imply that its source root contains items.

Approved Item close likewise validates the close requirement and Item source before removal (`358–397`). Its intervening successor selection changes only selection/MRU (`1686–1714`), preserving both membership and nonemptiness. The same implication therefore holds after successor selection. Close multiset reconciliation, canonicalization and final candidate handling remain unchanged.

Cleanup still performs the original root lookup **before** branching (`3595–3603`), preserving `MissingRoot` priority. A root with a central node still bypasses content cleanup exactly as before. If removal empties the source tabs, the original `subtree_has_items(record.node)` and root/presentation removal path still run unchanged: empty source with nonempty peers retains the root; completely empty noncentral source retires it. The shortcut neither retires a central root nor assumes that an empty source means an empty root.

No issuance, stale-reference check, completion, normalization or publication boundary was removed. Move and close still operate on their independent candidates; failure after extraction or successor selection cannot publish partial changes. Same-tabs checked no-ops still return before detachment.

## Existing meaningful coverage

- `operation/tests/local_item_move.rs:113`: matrix of selected/unselected removal, last/nonlast source, central source/peer/absence, same/cross-root and center/gap/edges; strict validation, exact MRU and item multiset checks. The local/general comparison shares this cleanup helper, so that comparison alone is not an independent cleanup oracle.
- The same file at `174`: actual last-item root/presentation retirement for main/contained sources, independent candidate discard and retry.
- `operation/tests/owner_reference.rs:282`: empty source with a nonempty peer remains in the staged root; later normalization and late-error rollback. Existing same-version issuer tests remain relevant to unchanged validation.
- `tests/tab_mru.rs:118,146`: approved selected close and selected Item move preserve the expected remaining selection/history.
- `tests/transaction_atomicity.rs:207,260,804`: post-extraction failure, later-command rollback, and approved last-item contained close with presentation cleanup.

These tests cover the important branches without a new test that merely asserts the boolean or mirrors the guard. Their execution belongs to Root.

The deleted work is a reachability/content query and its temporary Vec/HashSet when the source remains nonempty. The old query exits at the first nonempty leaf (`workspace.rs:948–963`); it is not necessarily a full-root traversal on every sample. No asymptotic or latency improvement is claimed.

Only static source/diff/hash and existing-test inspection were performed. No other review was read; no source, candidate or test was edited, built, run, profiled or measured.
