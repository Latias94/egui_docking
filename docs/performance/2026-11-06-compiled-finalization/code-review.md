发现 **1 个应修复后再合入的接受行为回归（P2）**。审查基于 `aba4c27b00746de929329fac0a0db3054448a7f6` 与实际工作树的差异，七个文件的哈希均与 state.json 一致，包含尚未跟踪的新增测试文件。

**[P2] 零内容区窗格退回无引导目标路径时，新终结化会接受旧校验器拒绝的计划。**
位置：[validation.rs:183](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:183)。

这里丢弃了 `validate_drop_records` 返回的覆盖状态，随后直接执行 `finish_validation`。被一并省略的 `target_coverage.is_complete()` 检查包含一个尚未由编译器保证的关系：每个中心／边缘目标必须由引导簇或磁性字段覆盖。

可以构造以下合法输入：默认配置和策略；主根为一个空的 central tabs；表面边界为 `(0, 0, 800, 28)`；两个 contained 根各含一个不同的 item，矩形分别为 `(0, 0, 400, 200)`、`(400, 0, 400, 200)`；提供匹配 manifest 的完整测量，标签栏预留为零。

沿现有代码可推导出：

1. 默认标签栏高度为 28，主窗格的内容高度变成零。两个浮动窗口各自只覆盖部分主根，但合起来覆盖整个主根，导致外部引导的可放置区域耗尽，返回 `None`。见 [surface.rs:2488](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_compiler/surface.rs:2488)。
2. 编译器进入 `compile_inner_targets_without_guide`，使用有面积的 `node_bounds` 替代零内容区，仍生成中心目标。见 [surface.rs:2916](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_compiler/surface.rs:2916)。
3. 磁性窗格字段的工厂却会因内容高度为零直接返回，不生成字段。见 [target_field.rs:224](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/target_field.rs:224)。此中心目标因此既没有引导簇，也没有磁性字段为其计入覆盖。
4. 完整校验器会在 [validation.rs:483](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:483) 返回 `InvalidMagneticTargetField`；新终结化跳过此检查，剩余检查及链接允许该计划成功。

这是正常编译器在外部几何输入下产生的输出，不需要修改原始计划或突破证书封装。建议保留这项目标覆盖关系检查，并加入上述输入的完整 `Result` 对比测试；可以遍历现有字段与引导簇的目标 ID，无需恢复全部磁性字段重建。当前新增生成测试固定只有一个浮动窗口，覆盖不到这种联合遮挡条件。

除此之外，所审差异中未发现其他阻断项：证书的私有构造、不可变借用和消费式终结化未见可错配或重新认证已修改计划的入口；通用完整校验器的可失败步骤保持原有顺序；窗格名册、分隔器 gap／rendered 配对和交点省略均有对应的编译发射逻辑支撑。

以上反例是静态代码路径推导，未执行验证。本次未修改文件、运行 Cargo／基准测试或调用子代理，也未独立复验所述测试通过数量和冻结校验器实验。
