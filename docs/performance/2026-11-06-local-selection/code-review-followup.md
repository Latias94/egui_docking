**上次 P2 已解决；本次限定增量无阻塞性发现。**

- [operation.rs:62](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/operation.rs:62) 的 `apply_draft_command` 先通过 `draft_mut()` 撤销资格，完整通用检查成功后才恢复资格；错误返回不会重新授予资格。journal 后续激活已接入该方法。
- [semantic_candidate.rs:299](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/semantic_candidate.rs:299) 保留混合 raise/select 的通用准备流程，通过 `into_snapshot_parts()` 保留成功结果资格。[pointer_timed.rs:719](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/pointer_timed.rs:719) 的纯选择也已接入快照方法及带资格结果。
- [新增资格测试](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/operation/tests/local_selection.rs:221) 的断言覆盖了“混合／后续编辑仍全检，下一条纯选择零 multiset 扫描”。generation 用例现在实际替换工作区节点后拒绝旧 source；帧回滚用例在选择成功后追加无效 MRU 替换并检查丢弃后的原状态。focus 断言也保留了直接命令与产品选择的既定区别。

剩余限制：本次仅做静态复核，未运行构建或测试；新增资格测试没有动态覆盖完整 journal／timed 交互链路。其他原始 `Workspace` 转换继续保持无资格，close/recovery 类型迁移属于已明确排除的范围。完整根 fingerprint 检查、事务候选深克隆和 changed 比较的成本仍然存在。