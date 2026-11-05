**静态审查通过，未发现阻断问题。** 已将指定六个文件与提供的 beforefiles 对照，并追查必要调用点。

- **排列来源与索引对齐成立。** 初始 `entries` 的 ID 完整覆盖 `0..N`；首次空间分区只重排行，复用后再次从当前条目的 ID 导出排列，因此完整性可沿生产路径保持。长度不匹配会重新分区。这里的 ID 属于 allocation/protected 临时条目，与压缩后的 manifest 行号无关；即使两类条目的数量分界改变，只要总数相同，旧排列仍能双射覆盖当前条目。[touch.rs:394](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:394)

- **没有沿用旧几何。** 复用分支重新生成所有当前条目，填入当前叶子边界，再自底向上重算父节点；非二次幂尾部保持 `EMPTY`。重叠判断、层级筛选、邻接行排序及密集回退保持原有逻辑。回退返回完整、自然有序的 `0..N`，不会使用构建到一半的稀疏结果。[touch.rs:416](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:416)

- **相等性、所有权和权限绑定保持正确。** `PartialEq` 只比较 regions/fragments，排除派生提示；新增 `Box<[usize]>` 不引入借用或内部可变性，也不改变原有 `Send/Sync` 条件。这条路径仅读取旧排列，新 manifest 仍绑定本次 output。缺少 layout facts 时，fragments 和 order 都返回空。[geometry_reuse.rs:10](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/geometry_reuse.rs:10)、[touch.rs:153](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch.rs:153)

- **完整 touch 编译仍在原测量边界内。** 新调用替换原调用的位置未变；protected 几何、allocations、BVH 边界、partition、遮挡扣减和最终 fragments 全部同步计算完成后才返回。调用仍位于 `install_ready → compile_reusing → compile_with_order` 内，没有把分片计算移到计时器之外。[presentation_hit.rs:441](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit.rs:441)、[lifecycle.rs:1264](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/lifecycle.rs:1264)

测试代码覆盖了 **288 条目这一非二次幂规模、反向排列、旋转排列、短提示重建**，逐行比较全部 allocation 的邻接列表，并验证输出排列完整；原分区测试继续与穷举结果对照。现有 manifest 测试还比较几何改变后的完整编译结果及 fragment 位模式。[touch_tests.rs:193](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/touch_tests.rs:193)、[geometry_reuse_tests.rs:182](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/geometry_reuse_tests.rs:182)

结论限于静态审查：等长提示的合法性依赖当前私有生成路径，代码没有为等长但重复或越界的外部输入提供容错；目前未发现这种输入的生产来源。旧排列的空间局部性也可能随布局变化变差，密集回退限制的是邻接存储，不能保证查询访问量。新增排列用例没有直接覆盖复用分支触发密集回退，现有相等性测试也未单独改变 `touch_order`。

本次未修改文件，未运行 Cargo、测试、剖析或测量；`e73d86a` 的既有 22 项门禁通过记录不代表当前改动已完成验证，当前结果仍待你安排的针对性及完整受影响测试确认。