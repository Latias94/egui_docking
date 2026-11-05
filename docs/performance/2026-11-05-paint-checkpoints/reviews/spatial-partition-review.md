**静态审查通过，未发现本次改动的阻断项。** 对照指定基线，差异仅涉及树的构造；查询重叠判断、层级过滤、邻居排序和 dense 回退逻辑保持一致。

设 allocation 数为 `A`，protected 数为 `P`，`N = A + P`，叶槽容量为 `L = N.next_power_of_two()`。

- **非二次幂长度的索引映射成立。** [切分逻辑](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:460)按子树叶槽容量切分：右子树非空时，左侧恰好占满 `half` 个槽；左侧不足时，右侧为空。因此可归纳得到 `bounds[L + i]` 始终对应最终的 `entries[i]`，与查询的 `entries[node - leaves]` 一致。例如 `N=5、L=8` 时，条目落在叶槽 `8..=12`，其余叶槽保持 `EMPTY`。选择操作的枢轴保留在右切片首项，没有遗漏或重复。

- **空输入与单条输入成立。** [初始化及调用](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:386)在 `N=0` 时产生 `L=1`、两个空 bounds 槽，递归直接返回，最终得到空的 Sparse。`N=1` 时根节点就是叶节点，写入边界后返回，不会调用越界的选择操作。

- **有限极端端点不会导致漏查。** [选轴表达式](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:466)中的跨度相减可能溢出为 `+∞`，但包围盒仍直接保存原始有限端点，没有通过跨度重建边界，因此不会向内舍入。两轴跨度同时溢出时会选择 X，这会限制选轴质量；`total_cmp` 加原始 row 的比较仍构成确定的全序。

- **半开重叠与处理顺序保持一致。** [重叠判断](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:351)继续使用严格 `<`，仅接触边缘不会成为叶级重叠。父节点覆盖全部子条目，重排不会产生错误剪枝。原始 row 身份随条目保留，最终邻居排序恢复处理顺序，层级条件和 dense 回退语义未变。

复杂度界限如下，均为静态推导：

| 项目 | 上界 |
|---|---|
| 树构造 | `O(N log(N+1))`；每层折叠与线性选择合计 `O(N)` |
| 构造递归与查询 pending 栈 | 各为 `O(log(N+1))`，空输入为常数空间 |
| 邻接构建阶段辅助内存 | `O(N)`；`N>0` 时 bounds 有 `2L < 4N` 个槽，邻接链接量受 `64A` 阈值约束 |
| 全部查询遍历 | 最坏 `O(A·N)` |

**dense 回退仍只约束邻接存储，最坏查询可以是二次复杂度。** 例如所有 allocation 矩形重合、层级两两不同且没有 protected：每次查询访问整棵树，层级过滤拒绝所有配对，`links` 始终为零，不触发回退，总访问量为 `Θ(A²)`。这是基线已有的限制；本次改动不能据此宣称查询具有 `O(log N + k)` 保证或已证实性能改善。

本轮仅作源码对照与静态推理，未修改文件，未构建、测试、剖析或测量。

工具调用简报（2026-11-05，UTC+08:00）：`exec_command` 读取基线、当前源码及本地 Rust 1.95 标准库；`tool_search`（结果数上限 2、1）定位代理工具；`spawn_agent`、`wait_agent`、`close_agent` 完成并关闭 1 名索引及复杂度专项复核，结论一致。等待收集超时 2 次后正常完成，无操作重试或退避，无外网来源。