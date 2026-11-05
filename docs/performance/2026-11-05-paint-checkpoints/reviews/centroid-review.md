**PASS：本次两文件 followup 未发现阻塞项。** 已对照指定旧快照完成静态审查，独立 Codex 子代理复核也未发现缺陷。

- **邻居完整性与短路语义保持。** 中心仅用于分组；节点包围盒、叶子包围盒和重叠查询仍使用原始端点。邻接行最终按原始行号排序，因此分区顺序、遇到 protected 行时的 `break`、`partition_point` 以及 dense 回退语义保持。[包围与查询实现](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:340)
- **中心计算与比较器安全。** 有限端点分别乘 `0.5` 再相加，不会溢出或产生 NaN。中心跨度相减可能溢出为 `+∞`，但仅影响选轴；次正规数舍入、正负零也只影响树形。`total_cmp` 加唯一原始行号构成一致的全序。[中心分割实现](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:460)
- **空树及非二次幂索引成立。** `split = min(half, len)` 保持子树容量不变量；存在右子树时左侧恰好占满 `half`，因此叶索引 `node - leaves` 仍对应重排后的条目。`select_nth_unstable_by` 受 `split < len` 保护，没有新增越界路径。
- **复杂度界未恶化。** 令 `N` 为全部条目数、`A` 为 allocation 数：新增中心扫描后，建树仍为 `O(N log N)`，辅助空间为 `O(N + A)`，递归及遍历栈为 `O(log N)`。查询最坏仍可达 `O(A·N)`；dense 阈值约束邻接存储，不能保证所有布局都避免二次查询。
- **测试改动有效，计时隔离保持。** `137` 与 `256/1024` 互质，置换保留全部几何行并消除了 ID 的空间顺序捷径，能够暴露旧策略按长轴同坐标、再按行号分组的退化。既有穷举分区对照完整保留；测试模块及访问计数均受 `cfg(test)` 限制，不进入产品计时。[结构回归测试](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch_tests.rs:303)

本次未修改文件，未构建、运行测试、采样或测量。此前 **22 项门禁通过**属于 followup 前版本；本结论支持继续既定流程，在最终测量后运行定向测试及全部受影响门禁。

工具调用简报：截至 `2026-11-05T23:35:34+08:00`，`exec_command` 用于只读差异和源码核对；`tool_search` 定位代理工具，`multi_agent_v1__spawn_agent/wait_agent/close_agent` 完成 1 名独立只读复核并关闭，返回 0 项缺陷。一次等待超时后收齐结果，无代理重试或退避。