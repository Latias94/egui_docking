未发现阻塞性的正确性问题。发现 **1 项非阻塞的性能与资格传递遗漏**，建议补齐。

1. **[P2] 部分已检查的成功发布会丢失 `selection_ready`，使下一条单独 `Select` 再次执行全局规范化与 multiset 扫描。**

   在 [engine.rs:3494](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine.rs:3494)，journal 后续激活通过 `draft_mut()` 撤销资格；随后 `apply_owned_draft_command` 虽然成功完成 prune、canonicalize 和 item reconciliation，资格仍保持为 `false`，并被原样发布。

   另一个实际入口是 contained 标签需要同时提升层级时：[semantic_candidate.rs:313](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/semantic_candidate.rs:313) 使用 `into_parts()`。该方法在 [transaction.rs:78](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/transaction.rs:78) 将 `CanonicalWorkspace` 降回原始 `Workspace`，发布时再经 `From<Workspace>` 转成无资格快照。因此，“提升窗口并选标签”成功后，下一次普通单条标签选择仍会走完整事务。

   这不会放行无效状态，但会让局部选择成本取决于上一条操作的发布入口。建议让无需继续裸改的成功事务使用 `into_snapshot_parts()`；对原地后续命令，在 owner 内部于完整检查成功后重新产出带资格结果。保留原始 `Workspace` 转换不授予资格的规则。补充“混合批次正常全检，随后单条 `Select` 无 multiset 扫描”的断言即可验证。

其余重点的静态核对结果：

- **固定点资格成立。** 严格 validator 拒绝孤儿节点、非 central 空叶、少于两个孩子的 split、同轴嵌套、无效 MRU 和空 surface；这些覆盖了 canonicalize 与 prune 的改写条件。未改拓扑的 split 会保留原权重，没有发现浮点重复规范化导致的差异。
- **资格封装未见越界。** 私有 `Arc`、无 `DerefMut`、`draft_mut()` 先清资格、原始转换默认无资格，以及通用事务所有检查成功后才构造 `CanonicalWorkspace`，这些边界一致。engine candidate 克隆保留原资格；restore 重新严格验证；native counterfactual 使用独立候选，正式提交仍重新执行检查。
- **选择语义未见回归。** 快路径只匹配精确一条 `Select`，继续使用原有完整 source 校验；根 fingerprint、节点 generation、成员资格及 MRU 更新没有被简化。混合批次仍走通用事务；revision、noop、事件和 focus 的发布边界未见改变。整帧仍在独立 engine candidate 上工作，最终提交才替换发布状态。
- **owner 封装复杂度合理，未发现新增的必然额外深克隆。** 常见选择路径仍深克隆一次事务候选；当前 `draft_mut()` 和 `into_workspace()` 调用处持有独占的新候选。部分预检路径新增了短暂的 Arc 分配与拆包，但不等于新增 workspace 深克隆。**完整根 source fingerprint 重建、workspace 克隆及 changed 比较的成本仍然存在。**

未验证的假设与覆盖边界：

- 按要求未运行 Cargo、测试或 benchmark；编译与运行结果尚未确认。
- 新属性测试随机化的是选择序列与锁定状态，拓扑仍是固定 fixture，不能作为所有拓扑固定点等价性的动态证明。
- generation 用例在独立 arena 中回收节点，再把新 ID 用于旧快照；尚未覆盖“实际工作区复用槽位后，旧 source 被拒绝”的完整过程。
- 新整帧用例覆盖丢弃 candidate，没有制造“局部选择成功后，后续阶段失败”的实际失败链；该原子性结论来自静态所有权与提交路径检查。