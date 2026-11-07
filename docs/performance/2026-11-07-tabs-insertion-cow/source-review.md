# Tabs insertion copy-on-write design

Recommendation: this is worth one bounded experiment after the current candidate is decided. Add one intent-specific private `NodeStore` method for a valid insertion into a shared, nonempty Tabs node, then retain the original `insert_items` body. It can copy each existing Vec directly into sufficient capacity and avoid the immediately following growth. Keep ordinary `get_mut` unchanged for every other caller and preserve its behavior for unique nodes, empty targets, wrong-kind nodes and invalid indices. This does not reduce the number of durable owners: a Tabs payload still has one `Arc<Node>` and two Vec allocations.

The design should remain roughly two runtime files and one private method, without new containers, dependencies, traits, allocator wrappers or generalized cloning framework. The current source evidence supports the location and mechanism, not an allocator-time share or an improvement forecast.

**Source and executable basis.** Runtime/control is immutable `c7508eec3ac44138305797691244ae9c180d7680` / `0e441c8`, not the live `operation.rs` being changed for source-nonempty.

| Artifact | SHA-256 |
| --- | --- |
| Immutable `operation.rs` | `7586ec06b29b77a794133e0c554d4fcca7a6faa16b8c550b86034ad394175862` |
| `graph/shared_storage.rs` | `18c6f75150643779acb9c71c30486634b19f7ccec5677e4cc64ce9043fcc532e` |
| `graph.rs` | `ea8aa5ffe7b4b7d67ad80cb289631e05b3da53a8cf01b6441d2d2de180f04951` |
| `graph/copy_tests.rs` | `fa5478d74b3e851fa74f26dfeb6d0f1bd2176b091ceca0bd3048697cc1be39c4` |
| Current-profile receipt | `ec94fad210c92a69b6d0d86060bcf40b592542f7a0857f81353a159fc13a0f01` |
| Center raw sample | `a704c5599f34f2f15912c626ffcbb2cbfa3cc9f9568e4fdb2072bca45d3ae242` |
| Diagnostic executable | `35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590` |

Source paths below are relative to `crates/dockspace/src/`. I read immutable sources, the existing copy tests and the current sampled binary statically. No build, test, execution, sampling, timing, network request or repository change was performed.

**Observed mechanism.** `NodeStore::get_mut` (`graph/shared_storage.rs:35–36`) performs a full-generational SlotMap lookup and `Arc::make_mut`. `Node::clone` (`graph.rs:192–215`) clones both Tabs Vecs independently. In this actual executable, the Tabs arm at `0x100745680..0x10074572c` allocates each nonempty Vec using its current length × eight bytes and records capacity equal to that length; this is a control-binary fact, not a permanent general guarantee about `Vec::clone`.

`insert_items` (`operation.rs:3242–3291`) immediately splices incoming items, then updates selection and MRU. The center sample has seven inclusive observations under:

`timed prepare → insert_payload → merge_payload_into_tabs → insert_items +800 → reserve`.

Static inspection identifies this particular reserve as the **visual items Vec**, not the MRU Vec: `insert_items::he45452b1ad7861dc` starts at `0x1009c3f50`; the call at `0x1009c426c`, returning at `+800`, passes the items descriptor at `node + 0x10`, current length and incoming count, with element size/alignment eight. The MRU descriptor is at `node + 0x28`; its front insertion has a separate `grow_one` call at `0x1009c44a8`. Thus both growth mechanisms exist, but the seven sampled observations do not measure both or count allocations. The saved edge workload has no `insert_items` path.

A normal center/cross Item move into two existing tabs copies the target's two items and two MRU entries, then needs capacity for three of each. The old source is still available to the prepared candidate; it cannot simply be consumed. Reserving after ordinary `Arc::make_mut` is too late to avoid clone-then-grow.

**One small interface.** A possible name is `NodeStore::get_mut_for_tab_insert(node, index, additional) -> Option<&mut Node>`. It returns the same kind of mutable payload as `get_mut`; it does not accept selection/history, execute a command, issue references or return `CommandError`. The additional index parameter is an optimization eligibility check, not a second command validator.

Its implementation should:

1. Use `self.slots.get_mut(node)` unchanged, so missing/retired full keys remain `None` and SlotMap allocator/iteration/free-list semantics do not change.
2. Specialize only when the Arc is shared, the current node is nonempty Tabs, `additional > 0`, and the insertion index is within the existing item length.
3. Copy the existing item and MRU slices into two ordinary Vecs with capacity at least their own `len + additional`, preserve selection verbatim, and replace only this slot's Arc with a fresh Tabs payload.
4. Finish through safe Arc mutable access. All other cases use the existing `Arc::make_mut` behavior without pre-reserving.

The caller replaces only its initial `get_mut(tabs)` call, passing the existing index and incoming slice length. The original MissingNode, NodeIsNotTabs and TabIndexOutOfBounds handling stays in order, followed by the same splice, selection and MRU code. No borrowed check is moved across source detachment or another mutation.

The nonempty restriction avoids a concrete cold-path loss: an empty central target receiving a whole-Tabs payload replaces its history with `source_mru.to_vec()`. Preallocating a throwaway empty MRU buffer before that replacement would add an allocation. Invalid indices and wrong-kind nodes should fall back as well, retaining the original clone/check sequence and work accounting on failures. Do not introduce a capacity-specific recoverable error or a new command-validation pass.

The method must not treat `Arc::get_mut == None` as proof of another strong owner: it can also mean weak ownership. Keep the standard `Arc::make_mut` path when the strong count is one, preserving its weak-detachment behavior. A strong-count observation is only a safe path-selection hint; if ownership changes concurrently, the standard operation remains the fallback, not unchecked uniqueness. No project `unsafe` is needed.

**What stays single and unchanged.** Selection/MRU semantics remain entirely in `insert_items`: selected item first, prior target order retained, other incoming items appended in incoming order, and whole-source history preserved on empty targets. The helper never inspects membership, invents a second merge algorithm, or “validates” MRU again. It merely copies the old vectors, so it does not need payload provenance, policy facts, root IDs or a public Tabs type.

Source Tabs detachment, Node payload extraction, outcome items, root issuance, placement repair and canonicalization remain outside this method. Ordinary center/cross insertion can benefit; the existing same-tabs Item fast path and new edge Tabs construction do not call it. Prepared candidates still own independent slot values, while the original workspace and any old readers retain their original Arc payloads. Success/failure publication, staged inspection, later-command checks and late rollback remain the existing transaction's responsibility.

**Copy accounting is part of correctness.** The new concrete Tabs copy bypasses `Node::clone`, so it must invoke the existing test-only `structural_work::record_node_payload_clone` once on the **original** Node, immediately before copying it. Do not invoke `Node::clone` and then reserve, or record the new larger Node.

This preserves one payload copy and precisely the old lengths of item/MRU entries. Newly inserted entries are not old payload-copy work. The shared center-move witness in `graph/copy_tests.rs:120–197` must still report two payload clones, four tab-item copies and four MRU-item copies; NodeStore forks/handle counts also remain unchanged. Unique followups record no payload copy. Wrong-kind/invalid-index fallback keeps the existing original clone path; no double-recording is allowed.

**Costs that could reject the experiment.**

- Two old Vec allocations and two old-slice copies remain necessary. The target Arc allocation, splice shifts, MRU front shift and source payload copy remain. Capacity planning can remove subsequent growth calls; it is not an ownership elimination or a measured allocator-count result.
- Shared/unique and eligibility dispatch add loads/branches. Safe mutable access to the newly created Arc can add another uniqueness operation. The existing generic `Arc::make_mut` already packages this work tightly; moving it to a helper can worsen code layout.
- Capacity exactly `len + incoming` reduces amortized headroom. A two-item target becomes length/capacity three, whereas this control's ordinary growth can leave capacity four. A second staged insertion may therefore grow earlier. Preserve standard growth for unique followups; do not hand-copy Vec's private growth formula to conceal this tradeoff.
- Large merges and unusual MRU sizes increase initial allocation requests. Allocation failure timing is not a new recoverable command contract; keep bounds checks on the original fallback path rather than allocating speculative extra space before a known index error.
- There is no sampled evidence here for resize or edge improvement, and no guarantee that seven inclusive center observations are enough to move complete-operation medians. A formal full comparison must retain all negative cells.

**Minimal meaningful verification, if Root implements it.** Reuse `center_move_copies_only_two_complete_tabs_payloads_and_unique_followup_copies_none`, late-batch-failure and slot-reuse copy tests (`graph/copy_tests.rs:120, 360, 395`), the complete-normalization/MRU differential suite (`operation/tests/local_item_move.rs:112`), and existing staged/owner-reference tests. The current unique followup is a Select; the only narrow coverage gap is a second **insertion** into an already private target. Add a short valid Open followup through the existing owned-draft operation in that test, checking zero new payload copies, exact visual/MRU contents and the old workspace's untouched contents. Do not assert capacities, raw addresses after growth or an allocator count.

This is sufficiently localized to trial without redesigning NodeStore. Defer it if it expands into a generic clone-policy abstraction, duplicates insertion/history logic, or needs more than the local store/caller change plus the one semantic followup. Retention still depends on the separately frozen, unchanged complete-operation/work/vector protocol; the design alone is not a winner.
