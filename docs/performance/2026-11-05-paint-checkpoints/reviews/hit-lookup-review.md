**审查开始时读取到的 HitLookup 版本静态通过，未发现阻断问题；结论有版本限制。** 收尾复核时，工作区已移除 `HitLookup`、恢复二分查找，并新增 `touch_order`，可见当前的 [presentation_hit.rs](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit.rs) 与 [geometry_reuse.rs](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/geometry_reuse.rs)。下面的结论仅针对此前实际读取的 HashMap 实现，不覆盖这些后续变更。

**规范轴：无阻断。**

- `HitLookupKey::Hash` 委托完整 `PresentationHitRegionId`，`PartialEq` 比较同一完整 ID；surface、节点 generation、junction 四向 arm 身份均未被截断，哈希碰撞仍需完整相等判断。
- 标准 `HashMap` 使用默认随机状态。索引排除于 `HitGeometry::PartialEq`，语义比较仍由 `regions`、`touch_regions` 决定，独立建表的随机种子不会改变相等结果。
- `cfg(test)` 计数位于真实键比较中。16/128/1024 用例在建表后重置计数，不会混入构建比较。

**需求轴：无阻断。**

- 索引在 canonical ordering、touch 编译及 compaction 后构建，保存最终 arena 行号；`region_index` 使用 map，未见压缩前行号残留。
- 索引与 regions、touch rows 同属一个 `Arc<HitGeometry>`。整体复用仍经过完整 region 字段及触摸输入上下文比较，并绑定本次 `output`；未见权限凭证与共享几何错配。
- `collect()` 遇到重复 ID 会覆盖旧行，但生产路径已有语义唯一性校验、program 操作去重及 guide target 排除。该前提有静态依据；索引本身不承担重复输入校验。
- 当时读取的三个 fixture 均从对应最终 arena 或其等值克隆建表，未见索引与数据错配。

结论的主要限制：

- 现有结构回归仅检查成功查询存在及比较次数预算，没有核对返回 ID，也未直接覆盖 foreign-surface miss、同槽不同 generation、各方向 arm 差异及压缩后逐 ID 查询。因此不能据此证明完整查找覆盖或 equality 次数降幅。
- 未测量实际加速、完整键存储、建表或最后一个 Arc 释放时的成本。派生 `Debug` 还会增加 map 输出，其遍历顺序可能变化。
- 指定 priorfiles 仅含三个文件；另外两处 fixture 按当时源码做一致性检查，未作精确前后差异比较。

全程未修改文件，未运行 Cargo、测试、性能分析或测量。

工具调用简报：`exec_command` 用于本地只读检索与差异阅读；`tool_search`（结果上限 3）发现子审查工具，启动两条只读审查并均已关闭。工具发现未重试，无外部来源。时间：2026-11-05 23:46:50 +0800。