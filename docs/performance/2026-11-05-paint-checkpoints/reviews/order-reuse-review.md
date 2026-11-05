**通过：对照指定检查点的四份原文件，未发现本次 canonical-order 复用变更的阻塞问题。** 静态代码支持你描述的边界：完整当前发射数量和逐项完整 ID 一致时，可以复用旧排列；只有既有完整几何复用 guard 通过，才共享旧几何。

- **缺失、增加、替换或重排 ID 会回退。** 新路径先要求 `emission_rows.len() == regions.len()`，再按当前发射顺序，将完整 ID 与旧规范行逐项比较。数量检查排除了 `zip` 只比较共同前缀的问题；任何不匹配都会调用原排序逻辑。[region_order.rs:12](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/region_order.rs:12)

- **映射方向正确。** `emission_rows[e] = c` 表示“发射行 → 规范行”；新增代码生成 `sources[c] = e`，符合原置换算法要求的“目标行 → 原始行”。抽取出的 `permute_regions` 保持原循环不变，移动的是当前完整记录。[region_order.rs:24](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/region_order.rs:24)

- **压缩不会留下失效排列。** 提示由规范排序生成，并且仅在压缩没有删除任何行时保留。当前 touch 编译和压缩仍在排列之后执行；当前若删行，新几何的提示就变为 `None`。索引安全依赖这个现有的私有、不可变排列不变量；它不是面向任意损坏提示的校验器。[presentation_hit.rs:438](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit.rs:438)、[presentation_hit.rs:603](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit.rs:603)

- **当前几何、touch 和输出权限的边界保持正确。** 完整复用失败后，`compile_with_order` 从当前计划重新发射记录，重新编译 touch，再创建新几何；旧几何只提供排列提示。两个成功返回分支均使用本次传入的 `output`，没有复制旧输出票据。既有完整共享路径仍要求 bounds、touch context、全部记录字段及完整发射数量一致。[geometry_reuse.rs:44](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/geometry_reuse.rs:44)

- **失败顺序及浮点位保持不变。** 当前 `emit_regions(...)?` 完成后才使用排列提示；完整复用探测也必须成功遍历后才能返回，压缩错误继续传播。排列仅交换记录，不重建矩形；完整几何 guard 继续比较 hit、anchor 端点的 `to_bits()`，能区分正负零。[presentation_hit.rs:438](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit.rs:438)、[geometry_reuse.rs:24](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/geometry_reuse.rs:24)、[region_order.rs:50](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/region_order.rs:50)

一个**非阻塞的测试覆盖边界**：新增成功复用案例采用逆序排列，该排列恰好自逆；现有非自逆三循环案例直接覆盖的是原排序入口。增加成功复用三循环案例，可以更直接地防止未来把映射方向写反。本次静态推导未发现该错误。[order_tests.rs:105](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/region_order/order_tests.rs:105)、[order_tests.rs:196](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/region_order/order_tests.rs:196)

本结论仅基于指定检查点差异及相关调用链的静态阅读。未修改文件，未运行 Cargo、测试、性能分析或测量；测试源码中的断言不代表执行结果，性能收益也未验证。