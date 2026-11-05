**结论：PASS（静态正确性审查）。未发现相对指定旧版的正确性阻断项。** 对比基准为 `/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/pre-touch-bounds-tree/` 中的两个原文件。

设 allocation 数为 `A`，allocation 与 protected 总数为 `N`，核对结果如下：

- **邻居集合与层级谓词一致。** allocation 对仍要求包围盒重叠且层级相等；`first_id < second_id` 使每对仅登记一次，再双向加入邻接表。protected 仍要求其层级 `<=` 当前 allocation 层级，与旧扫描中两种排列方向的谓词等价。父节点包围盒包含所有后代，剪枝不会遗漏满足叶节点重叠条件的记录。[touch.rs:423](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:423)

- **邻居顺序与 dense 行为一致。** 最终仍按原始编号排序，因此 allocation 对处理、base 恢复和 protected 减法的顺序保持不变。新旧版本登记的链接总数相同，故 `links > 64A` 的回退判定也相同；dense 返回相同的升序完整编号列表。[touch.rs:431](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:431)

- **半开边界、极端坐标及 `+0/-0` 语义保持。** 叶节点沿用原来的严格 `<` 重叠条件，边缘相接仍被排除；零面积范围可能产生的保守候选行为也未改变。父节点仅对已有端点取 `min/max`，没有通过 `max-min` 重建矩形，因此不引入极端有限坐标的溢出或远端内缩。合法矩形端点排除了 NaN/无穷；有符号零可能影响内部端点位型，但不改变重叠比较结果，内部包围盒也不参与最终片段坐标计算。[touch.rs:323](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:323)

- **空输入、索引和辅助空间有界。** `N=0` 时 `leaves=1`，分配两个槽且没有查询，返回空 `Sparse`；只有 protected 时也返回空行表；单叶根索引和补齐叶节点均安全。对 `N>0`，令 `P=next_power_of_two(N)`，树有 `2P<4N` 个槽，DFS 栈最多 `log₂P+1` 项。稀疏链接数不超过 `64A`，触发回退时最多暂存 `64A+2` 项；包括 dense 返回前的瞬时峰值，辅助空间仍为 `O(N+A)`，新增树带来线性空间常数。[touch.rs:393](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:393)

**性能存在明确的非阻断边界：** 排序为 `O(N log N)`，建树为 `O(N)`，单次查询最坏仍为 `O(N)`，全部查询最坏为 `O(AN)`。例如所有 x 区间相同，把 protected 按“位于全部 allocation 下方、上方”交替排列，其内部包围盒会跨过所有查询，而叶节点全部不相交。此时可以出现二次访问量，同时 `links=0`，不会触发 dense。dense 阈值约束邻接存储量，不约束无效候选遍历耗时。[touch.rs:405](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:405)

新增回归的设计能够约束 **256/1,024 个按 y 递增的长行布局**：要求空邻接表、保持 `Sparse`，并限制查询节点访问数。它主要覆盖 x 无法排除候选时的 y 剪枝；`<80A` 是这两个规模的预算，不能推广为任意排列下的线性或次二次保证。计数器也不包含排序、建树和分配成本。[touch_tests.rs:303](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch_tests.rs:303)

全程未修改文件，未运行 Cargo、测试、profiling 或测量。上述 PASS 来自源码等价性推导；独立子代理复核了复杂度、空间边界及新增回归，不能替代编译、测试或实际性能证据。

工具调用简报（2026-11-05 21:49–21:56，Asia/Shanghai）：`exec_command` 只读查看新旧源码、差异及几何约束；`tool_search`（结果上限 2/1）发现子代理工具；`spawn_agent` 派发 1 名只读复核，`wait_agent` 收集结果后由 `close_agent` 关闭。两次等待超时，无失败重试或退避；来源均为本地文件。