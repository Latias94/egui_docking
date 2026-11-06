**还有优化空间，现有数据不足以把约 12ms 认定为架构或理论下限。** 两项候选都可以在你限定的范围内保持正确性，但主要是小幅节省；更大的剩余预算在编译、校验和安装中的重复遍历、排序及清单构造。

审查固定在 Git 对象 `f4b12d7`，以下行号以该提交为准；未构建、运行测试或基准、修改文件、启动代理。给定数据是独立中位数，下面仅用于判断优化预算，不相加嵌套跨度，也不直接从总耗时扣减。

**候选 1：建议先做，未发现限定条件下的正确性阻碍。**

现有 [`workspace_index_for`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/presentation_authority.rs:103) 已同时检查 workspace 的精确 `Weak/Arc` 来源和 index version。可把它作为复用语义快照的依据：[`draft_mut`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/operation.rs:53) 经过 `Arc::make_mut`，同版本草稿修改也会使原来源检查失效；`publish_workspace` 先换 workspace、稍后才推进 revision 的窗口同样会 miss。

需要守住四个边界：

- **原生窗口投影：** 缓存表确实混有尚未发布的 child 快照。必须按实际 `workspace.surfaces()` 逐键取值，缺任何实际 surface 就整次退回 live capture。直接克隆整个缓存表，会让已取消的 prospective child 残留；只做部分过滤又继续使用，会隐藏清单缺失。现有混合发生在 [`presentation_authority.rs:286`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/presentation_authority.rs:286)。
- **恢复与输入：** 复用只能替换快照捕获，仍须执行 source recovery 的 saga 撤销、native admission reconcile，以及 readiness、policy、popup、influence 等原有判断。输入 checkpoint 的 known/unknown 状态不能由这个缓存命中来证明。
- **checkpoint/restore：** 编码 checkpoint 克隆、恢复的是整个 presentation owner，因此 map 与 provenance 会一起恢复；workspace 替换则产生新 allocation/epoch，走 miss。见 [`host_frame.rs:1040`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/host_frame.rs:1040)。
- **发布一致性：** 继续在 owner 内同步发布新 manifest/index 来源和 semantic map；不能在 `publish_workspace` 时提前更新来源，却保留旧 map。现有发布位置是 [`presentation_authority.rs:479`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/presentation_authority.rs:479)。

其收益预算只是 **0.417ms 快照捕获中被避免的两次**，不是整个 1.484ms refresh。快照克隆共享 fingerprint 的 `Arc`，但仍有 surface/contained 条目复制成本；idle 本来不进入 refresh，所以不能宣称改善该 idle 路径。

**候选 2：范围安全、实现可以很小，但应以净收益决定是否保留。**

[`presentation_structure_eq`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/command.rs:57) 检查根及节点 ID、顺序、items、轴、权重、central 和 owner，只忽略 selection/MRU；这比 [`RootLayoutTopology`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/layout/topology.rs:9) 实际需要的结构条件还严格，因此是保守复用。

做法应限于：**先构造新索引及新完整指纹，再从旧的已初始化 slot 克隆 topology `Arc` 到新 slot**。旧 slot 为空就跳过，避免为无收益的根做完整比较；不复制旧 semantic index、测量值或几何结果。保留 [`layout.rs:780`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/layout.rs:780) 的 live `matches` 防线后，即便旧 slot 是来自同版本草稿的失配 hint，也只会退回冷路径，不会变成语义授权。

两个容易误算的地方：

- 新旧指纹比较仍扫描结构，暖路径还要 `matches` 和实时 `measure`。
- 冷 `compile` **同时完成首次测量**，所以 0.198ms 不是纯拓扑构造成本，不能全部算作节省。

因此它适合作为局部优化；若扩展成独立缓存、失效历史或提前预热机制，就超出了这点预算值得承担的复杂度。

**其余机会按投入价值排序：**

1. **先压缩校验器内的重复清单。** 3.251ms 的阶段中，[`validate_semantic_uniqueness`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:485) 排序临时 ID，pane/tab 校验又构造多组树表，最后 [`canonicalize_ready`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:1740) 再排序记录。可沿用已有 `DropTargetCatalog` 的方式，让本次校验建立的目录服务于重复检查、关联查询和最终顺序，同时保留稳定错误顺序。编译与校验还各推导一次 splitter junction；这是重复计算，但不能直接拿编译结果证明自身正确。

2. **单独拆解 measurement-change 的安装慢路径。** [`install_ready`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/lifecycle.rs:1264) 包含 hit manifest、paint-program 检查和 semantic manifest。现有 [`compile_reusing`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/geometry_reuse.rs:40) 在 bounds/touch context 或区域变化后会重建几何；失败的 replay 也可能先遍历一次、再重新 emit。3.624ms 值得优先分解，但目前不能断言全部来自 touch，也不能假设 selection 的 0.719ms 路径代表它。

3. **减少需求清单重建与树查询。** [`requirements.rs:151`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_compiler/requirements.rs:151) 为**每个 item** 请求 pane minimum，随后布局才选择当前选中项的答案。`SurfaceRequirements` 内部拥有集合和 map，manifest 仅共享外层 surface map。具体机会包括：只改 ticket/popup 时共享库存；[`draft_revisions_changed`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/presentation_authority.rs:454) 只调整 revision 时直接改 ticket，避免重新派生库存；把 [`exact_set_validator`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_manifest.rs:1441) 的双向逐项树查找改成有序合并。selection 可能改变 tab-bar policy，不能无条件复用该叶全部需求。

4. **最后再考虑事务所有权表示。** [`operation.rs:78`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/operation.rs:78) 已有 preserving selection 路径，但仍深克隆 workspace，再进行 live source 检查。精确来源证明下复用现有源指纹、缩小复制范围仍有空间；不过当前事务只有 0.249ms，其中 source checking 约 0.189ms，为此全面改成持久化结构的优先级较低。

令 \(N\) 为节点数、\(T\) 为全部 item 数、\(k\) 为当前叶的 tab 数、\(Q\) 为要求的测量答案数、\(K\) 为完整输出记录/程序步骤量、\(A\) 为实际变化的显式矩形数：

| 工作 | 可辩护的必要成本或边界 | HEAD 中的额外成本 |
|---|---|---|
| selection 修改 | selected 写入是常数；当前 Vec MRU 最坏 \(O(k)\) | 全 workspace 克隆、源根扫描；不是 selection 本身要求的全局下限 |
| 无碰撞命令指纹 | 新建完整扁平根快照需 \(\Omega(N_r+T_r)\)；独立快照精确比较最坏同阶 | 重复捕获可共享；无碰撞要求不禁止不可变结构共享 |
| 完整测量输入 | 本契约 \(Q\) 包含所有 item；接收任意全新答案需 \(\Omega(Q)\) | 库存重建、逐项 BTree 查询不必同时发生 |
| 测量与几何传播 | 显式更新需 \(\Omega(A)\)，最坏 \(A=\Theta(N)\) | 最小尺寸与 policy 不变的 selection 可以不重排；当前仍完整测量、投影 |
| 完整 scene 生成与校验 | 生成全新 \(K\) 条显式记录需 \(\Omega(K)\) | 多轮排序、清单重建和重复推导；不能据此宣称整体只有线性成本 |
| 完整 host recording | 契约要求逐步执行时至少 \(\Omega(K)\) | 查询、包装、分配和重复遍历的常数仍可降低 |

这里也不能把一般上界写成统一的 \(O(N)\)：例如 tab 校验对每个 member 在线性查找 visible record，存在单叶 \(O(kv)\) 项，见 [`validation.rs:1245`](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene/validation.rs:1245)；高扇出布局还有多轮 redistribution。持久化或分块所有权能减少修改时的复制，却不会消除完整输出遍历。

**能证伪“已经到下限”的实验**是：在相同完整输出契约下，做配对 A/B，保持程序步骤、接收器、几何和授权行为一致，比较端到端分布，而不是扣减各阶段中位数。

- 固定 minimum、policy 和 bounds，仅切 selection；检查候选 1 是否把重复捕获从三次降到一次，候选 2 是否避免冷拓扑构造且仍实时测量。
- 再加入选中项 minimum 改变、policy 改变和窗口尺寸改变，验证确实需要全局传播的分支；覆盖 native 取消/恢复、同版本草稿修改、发布早于 revision 和 checkpoint 恢复。
- 扫描不同规模，记录分配、排序量、节点访问量及输出量。若目录复用等局部修改在同等行为下稳定降低端到端耗时，就已证伪“当前毫秒数不可突破”。空 sink 只能测遍历基线，不能替代完整 recording。

复杂度只能约束工作量随规模怎样增长，**不能证明 1024 leaves 必须花 11.81ms 或 12.317ms**；完整 ImGui/egui 绘制耗时也不能充当 docking-only 的下限证据。