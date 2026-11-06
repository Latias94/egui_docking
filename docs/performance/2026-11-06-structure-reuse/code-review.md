**未发现阻断项，也未看到这次实现引入具体的正确性或错误顺序回归。** 已检查相对 `f4b12d7` 的实际 diff 和新增 `snapshot_tests.rs`；有一处非阻断的测试薄弱点，以及尚待测量确认的性能成本。

- **快照复用边界正确。** [`surface_semantic_snapshots_for`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/presentation_authority.rs:122) 先验证精确 Arc/version，再遍历全部 live surface；`collect::<Option<_>>()` 保证缺一项就整体 fallback。额外 prospective 条目不会进入结果。同版本 `draft_mut` 会使原 Weak 来源失配。新增测试也检查了缺项后的实际 rebuild 能恢复完整 map，而不只检查 helper 返回 `None`。

- **拓扑转移使用实际 topology guard 是必要且正确的。** [`build_reusing_layout`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/workspace.rs:137) 先生成完整新语义索引，再匹配旧 slot 本身，因此不会把“旧指纹仍匹配、slot 却来自修改后草稿”的 hint 填进新锁；[`对应反例测试`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/layout/topology_tests.rs:200) 确实覆盖了这一点。items、selection、MRU、owner 改变后仍可能共享 topology，这是安全的：topology 不保存这些语义，新索引仍捕获它们。

- **错误顺序、原生投影与 checkpoint 路径没有发现新增破口。** fresh build 仍发生在原索引构建位置，错误仍包装成同一个 `SceneRequirementDerivationError::WorkspaceIndex`；暖测量使用原有后序顺序，answers、metrics 和 projection 都重新计算。recovery/native/admission/stamp/policy/popup/influence 流程保留。checkpoint 恢复整个 presentation owner，map 与 provenance 一起恢复；共享 `OnceLock` 的填充只影响加速数据。转移的 topology 也没有指向旧 workspace/index 的引用，未见新增完整 workspace 滞留。

- **低优先级测试问题：** [`topology_tests.rs:304`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/layout/topology_tests.rs:304) 的 `if let Ok(refreshed)` 会吞掉非预期构建失败，使合法的轴、权重、central 或顺序变更也可能跳过新增断言。建议仅对删除节点的无效案例接受预期错误，其余案例明确要求构建成功。这是测试覆盖薄弱点，不是已发现的生产缺陷。

- **性能净收益仍未证实。** 转移时每个已初始化根增加一次 `matches`，正常测量时还会再检查一次。尤其是只重建 requirements、随后没有编译该根的路径，会承担额外扫描；冷 `compile` 原本还包含首次测量，不能把其全部耗时算成节省。见 [`转移检查`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/workspace.rs:148) 和 [`测量检查`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/layout.rs:780)。

剩余未验证的是最终 gates、最后新增缺项断言的运行结果，以及实际性能变化。新增单测没有直接执行真实 native saga 撤销/重试或 checkpoint 往返；这些边界目前的判断来自静态调用链核对。本次未运行构建、测试或基准。