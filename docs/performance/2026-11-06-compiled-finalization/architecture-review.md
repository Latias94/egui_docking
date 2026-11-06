**建议优先做这个接口重构：让编译器完成计划的检查与终结，生产路径不再运行通用验证器的期望数据重建。** 这里确实存在可删除的重复推导；但当前编译器尚不能保证“返回 `Ok(plan)` 就一定通过验证”，因此需要把少量几何与来源约束前移，不能只增加一个私有 newtype。

本次基于 `aba4c27b00746de929329fac0a0db3054448a7f6` 做静态审查，指定五个文件与基线一致。未修改文件、运行 Cargo／基准或启动代理。下面的反例由代码分支推导，未执行复现。

当前两条生产路径都很明确：

- 普通贡献在 [`prepare_surface_contribution`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/surface_contribution.rs:346) 中先编译，再创建 `PresentationPlanValidator`，运行 `validate_and_canonicalize`。
- 原生首帧投影在 [`compile_with_state`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/native_projection.rs:65) 中，对临时 workspace 重复同样的编译、验证流程。

编译器已经消费精确测量集合、生成结构身份与几何；验证器随后又遍历拓扑重建期望数据。其中最明确的重复是：

| 工作 | 当前重复的位置 | 建议归属 |
|---|---|---|
| pane 身份、选择项与层级清单 | 编译遍历叶子后，[`expected_pane_records`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:958) 再遍历一遍 | 由已检查的 workspace 和唯一叶子构造路径保证；完整重建留在测试对照中 |
| splitter gap／rendered splitter 集合 | 编译按 split 的 children 发射后，[验证器](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:1519) 再推导全部身份集合 | gap 与可见 handle 在同一发射操作中生成 |
| junction 几何及 operability | [`compile_splitter_junctions`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_compiler/surface.rs:272) 已计算，验证器又调用同一个候选推导函数 | 生产只推导一次；验证器保留独立比较 |
| 磁吸场的节点深度、root／pane 清单、边目标与覆盖关系 | 编译生成字段后，[`validate_magnetic_target_field`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:243) 再构造多组映射并重新计算 | 在发射 pane、root、target 时共同建立 |
| contained minimum／occlusion 清单与层级 | 都来自同一个 contained roster，又重新集合化检查 | 由一次 roster 枚举保证；投影来源与几何检查保留 |

这些工作主要是在复核编译器是否正确实现了自身协议。对封闭的内部构造路径，可以把保证建立在构造过程中，无须每次重新恢复“编译器应当生成什么”。

**必须保留的检查，可以按保护对象拆开。**

| 保护对象 | 具体检查 | 放置位置 |
|---|---|---|
| 任意宿主测量 | ticket、精确键集合、缺失／多余答案、`Unavailable` | 保留 `manifest.validate_surface()` 与 `require_authoritative()` |
| 宿主数据参与运算后的几何 | 聚合溢出、正面积、矩形包含、裁剪结果、滚动区间、关闭按钮与拖动区域分配、popup 可分配性 | 在几何生成处检查，或做一次不重建期望集合的局部扫描 |
| 内部 workspace／index 不一致 | workspace 的有效状态、index 版本与来源配对、节点存在与归属、override 来源与权重合法性 | 在编译上下文和布局输入入口建立 |
| contained 临时投影 | floating 所属 surface／root、`source_rect` 与当前记录一致、目标矩形正面积 | 从验证器前移到编译上下文 |
| 晚到的授权变化 | base stamp、policy、坐标关联、当前 contained projection、popup、原生 saga／resource／ready basis | 保留准备与提交阶段现有检查 |
| 发布所需的数据形式 | 规范排序、必要的 target availability 处理、`SurfaceProgram` 链接 | 保留独立终结步骤 |

这里的 `AuthoritativeSurfaceMeasurements` 证明的是答案完整且有测量值。它不证明这些数值能够组合成合法布局。[记录构造器](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/records/tab.rs:364) 也只是存储字段，并不检查这些关系。

另外，`WorkspaceIndex::at_version()` 只检查版本相等，并不证明传入的 workspace 就是该 index 的来源。现有 [`RootLayoutTopology::matches`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/layout/topology.rs:163) 明确防御同版本内部修改。除非新接口真正封闭了 snapshot／index 的配对来源，否则不能把这些检查一并当作冗余删除。

**当前至少有以下两个合法外部输入反例，以及一个内部投影反例。**

1. **空 central tabs，合法 reservation 产生越界视口。**

   使用有效的空 central tabs workspace，surface bounds 为 `(0, 0, 800, 600)`，默认配置，宿主提交 `TabStripMetrics::new(1000.0, 0.0)`，其余答案完整、ticket 正确。

   [`tab_strip_optional_chrome_candidate`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_compiler/surface.rs:565) 得到 `content_x = 1000`、`base_viewport_width = 0`。由于 `item_count == 0`，最小 tab 宽度检查不适用，于是接受视口 `(1000, 0, 0, 28)`。后续空 tab 循环和 gap 循环不会阻止编译返回。

   验证器在 [`rect_contains(bar.bounds(), bar.viewport())`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:1133) 处返回 `InvalidTabBarRecord`。零宽矩形仍然有位置约束，不能仅检查宽度非负。

2. **合法的 8 点 tab 宽度配置，产生 `Visible` 却没有对应记录的成员。**

   有效单 tab workspace，bounds 同上；配置只修改 `tab_min_width = 8`、`tab_max_width = 8`，其余保持默认，策略允许关闭。这能通过配置构造检查。

   编译分配宽度 8，完整 tab 位于视口内，因此先将成员标为 [`Visible`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_compiler/surface.rs:1256)。随后 [`operable_tab_chrome`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_compiler/surface.rs:1771) 将默认 16 点关闭按钮缩至 8 点，关闭按钮占满 tab，拖动宽度成为 0，于是返回 `None`，没有生成 `TabRecord`。编译仍可完成。

   验证器明确拒绝 [`(Visible, None)`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:1242)，返回 `InvalidTabBarRecord`。这需要在局部构造中处理，不能随期望清单检查一起删除。

3. **内部 contained projection 的来源字段不一致。**

   对有效 contained workspace，提供正确的 `surface`、`floating` 和正常目标矩形，但填入错误的 `root` 或过期 `source_rect`。

   编译器选取投影时[只匹配 surface 与 floating](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_compiler/surface.rs:108)，可以正常生成记录；验证器的 [`with_contained_projection`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:90) 则会拒绝。这属于内部输入来源检查，应当前移，不能仅转成测试断言。

前两个反例不需要损坏 workspace、伪造计划或使用 NaN。它们说明剩余检查必须包含几何关系与发射一致性，仅保留有限值检查仍然不足。

**我建议的内部接口只有一个生产编译入口、一个私有草稿阶段。**

入口可以收敛为：

`compile_surface_measurements(受约束的编译上下文, 宿主测量) → 已终结的 PresentationPlan`

上下文由普通引擎状态和原生投影各自的受控路径创建，绑定同一来源的 workspace、manifest／index、冻结策略与配置，并完成 contained projection 等临时输入检查。不要提供一个让任意调用方重新组合这些参数的通用构造器，也不要仅凭版本号宣称配对成立。

内部 `SurfacePlanDraft` 只由完整编译流程生成，终结方法消费草稿。它不接受任意 `PresentationPlan`，不提供 `trusted = true` 或跳过验证的开关。终结后的计划保持只读。证明来自以下构造约束：

- 每个有效叶子只经过一次 pane 发射路径，选择项和层级直接取自该上下文。
- 每个 split 的 gap、可见 handle 和共享 resize facts 在同一局部操作中产生；数值与几何约束在那里检查。
- target 身份、destination、guide 使用的共享 `Arc` 和磁吸场引用共同建立，减少独立传参造成的不一致。
- 全部 splitter 完成后推导一次 junction，并完成一次遮挡 operability 计算。

这不要求为每个矩形建立类型体系。把现有几何函数改成返回经过局部检查的结果，并封闭任意追加记录的入口，就能形成有实际保证的接口。

终结步骤仍要保留 [`canonicalize_ready`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:1753) 中真正改变数据的工作。特别是：

- **规范排序必须保留。** 记录查询与链接依赖排序；绘制顺序继续由原来的程序记录表达。
- **链接必须发生在最终排序之后。** [`finish_surface_program()` 与 `link_surface_program()`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/program.rs:710) 是不同阶段，前者完成构造程序，后者才把身份解析为最终行位置。
- **availability 要拆分处理。** 通用路径需要把任意旧 target 规范化为 `Stale`／`PolicyDisabled`，必要时重绑 guide。编译器在冻结上下文中刚捕获的 target，已经通过 `tab_availability`／`edge_availability` 设置状态；来源保证建立后，可省去再次检查这些新 target 的来源和结构。首版保留此遍历也可以，主要收益仍来自删除期望数据重建。

排序附带的重复身份检查、链接时的程序结构检查，首版可以继续保留，无须同时追求删除所有断言。

**最小实施顺序建议如下。**

1. 从通用验证器中抽出规范排序与链接，保持现有行为；把生产仍使用的 `region_has_authoritative_area` 等辅助函数移出验证器实现，不能直接将整个 `validation.rs` 标为 `cfg(test)`。
2. 封闭编译上下文，前移 contained projection 来源检查，保留测量入口、override 和布局检查；补齐上述局部构造缺口。
3. 引入私有草稿终结流程，让普通贡献与原生投影统一获得已终结计划；去掉两处生产通用验证器调用。
4. 通用验证器保留完整的 expected-pane／splitter／junction／magnetic-field 重建，作为测试对照。对同一草稿分别走快速终结和通用验证，比较接受结果、规范记录与链接程序；覆盖空 central、reservation 超宽、极小配置、裁剪、popup、contained 和 resize。

提交时的授权检查继续保留。[`reduce_surface_contribution`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/presentation_authority.rs:1145) 的 stamp／policy／坐标／投影复核，以及原生首帧的来源复核与命令重新执行，保护的是编译之后发生的状态变化。编译结果携带的保证只适用于它的输入快照。

按你提供的计时，目录优化后验证器约剩 **2.9 ms**，约占完整选择流程的 **26%**。这是值得优先处理的耗时范围；实际收益需要扣除仍保留的局部检查、排序和链接，由宿主测量确认。

若继续推进编译器本身，另一个具体位置是 [`compile_root`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_compiler/surface.rs:680) 每次重新建立的 `node_depths`：深度属于拓扑信息，可在已检查的 `RootLayoutTopology` 中保存并复用。不过，从现有证据看，先消除通用验证器对刚生成计划的完整重建，更符合这次需要的改进幅度。