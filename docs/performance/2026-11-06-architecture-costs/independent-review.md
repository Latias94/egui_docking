**核心所有权改动 PASS；诊断文档有 1 项 P2 证据表述需要修改。** 未发现此次 `Arc<BTreeMap>` 改动引入版本、authority、popup 隔离或历史输出行为回归。

- **P2：应限定“新 scene 急切构建全部设备视图”的适用条件，并承认已有几何复用。**  
  位置：[诊断报告:29](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-architecture-root-causes.md:29)，相关表述见[报告:172](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-architecture-root-causes.md:172)及[报告:177](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-architecture-root-causes.md:177)。

  实际安装入口调用的是 `PresentationHitManifest::compile_reusing`。它核对 bounds、touch context 和逐条重新发射的 region；全部一致时直接共享旧 `HitGeometry`，其中包含 pointer regions 与 touch fragments，仅绑定新的 output。证据见 [geometry_reuse.rs:40](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/geometry_reuse.rs:40) 和 [geometry_reuse.rs:61](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/presentation_hit/geometry_reuse.rs:61)。因此，新 output／新 scene 并不必然重新生成 touch 几何。

  **必须修改的证据陈述**是第 29 行的无条件概括。建议改为：“在现有精确几何复用未命中时，安装阶段仍会在首次查询前构建 touch 几何。”正文同时补充现有复用条件，将后续优化对象限定为复用验证中的重复遍历、复用失败后的急切派生，以及可能扩大的复用范围。resize 改变几何后仍可能承担完整派生成本，这部分诊断成立。

其余关键机制核验结果如下：

- **冻结清单共享成立。** 唯一清单构造入口 `Draft::finalize` 在既有验证和 popup 标记完成后执行 `Arc::new`；唯一冻结表变更入口 `with_popup` 在 popup 不变时直接返回，变化时通过 `Arc::make_mut` 分离共享数据。[scene_manifest.rs:821](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/scene_manifest.rs:821) 中的 authority、workspace、config、policy、revision 和 ticket 均未改变。`PartialEq/Eq` 保持值语义，迭代顺序仍由 `BTreeMap` 保证；字段私有，公开接口未变。新增指针相等断言只用于证明共享，没有新增生产缓存判断。
- **回滚、native 投影和历史输出语义保持。** native 的 `clone().with_popup(...)` 现在通过 COW 隔离保留快照；清单及行数据没有自定义 `Drop` 副作用，变化的是共享存储的释放时机。历史输出继续由独立的 plan、output ticket、popup 与坐标 authority 校验约束，未改为依据清单地址授权。
- **selection 根因链成立。** selection 进入完整事务准备，执行 workspace 深拷贝、item multiset、canonicalization、完整验证和候选比较；selection/MRU 又进入 root fingerprint，触发全局 requirement/index 派生及受影响 surface 的 Ready 失效。该 surface 随后的测量贡献进入完整编译路径；未受影响 surface 的 ticket 可以保持有效。报告没有把局部选择误称为所有 surface 全部失效。
- **scene validation 没有重跑完整数值布局 solver。** 它重建预期 roster、临时映射和集合，并执行几何关系、splitter/junction 等检查，不能将这些工作全部称为无意义重复。闲置路径还存在[编译前的完整输入复用](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/engine/surface_contribution.rs:285)，随后 `install_ready` 可保留相同 plan。报告提出的编辑日志、结构编译表、统一索引和延迟派生，属于尚待验证的重构建议。

分配证据通过核对：两份 headless CSV 各 180 行，工作负载及输出状态列一致；`allocation-comparison.json` 的 90 条记录均与摘要值和计算结果一致。1,024 leaves 下报告的五组分配事件数均正确：

| 阶段 | 基线 → 候选 | 减少 |
|---|---:|---:|
| Idle update | 4,840 → 2,793 | 2,047 |
| Idle settlement | 3,871 → 1,824 | 2,047 |
| Select update | 116,451 → 114,405 | 2,046 |
| Select settlement | 3,879 → 1,832 | 2,047 |
| Measurement-change update | 69,903 → 67,856 | 2,047 |

Idle 两阶段各减少 **551,896 requested bytes**；Select update 减少 **551,856 bytes**，与新清单增加一次 Arc 分配的代码机制一致。CSV 散列匹配元数据，两份各 363 项的源码散列清单仅 `scene_manifest.rs` 不同；当前清单与 harness 源码匹配候选记录。

冻结计时的两组 ABBA 共 8 个进程、240 行摘要；每种条件各 4 个进程。报告中的区间核对正确：

| 1,024 leaves，进程所报 p50 范围 | 基线 | 候选 |
|---|---:|---:|
| Idle update | 1.896–2.049 ms | 1.851–1.884 ms |
| Idle settlement | 0.567–0.601 ms | 0.505–0.517 ms |

Select 和 measurement-change 的 update 区间确实重叠。**这里只核对了各进程已报告 p50 的范围，没有重算逐样本 p50，也没有合并进程百分位。** 原 `frame_cost` 只输出进程内 p50/p95/max，未保存逐样本值。

除上述 P2 外，未发现报告将结果无依据地推广为产品整体性能、Rust／其他语言优劣、GPU/native 呈现或真实输入延迟结论。已有测试按提供结果及验证记录采信，本轮未重跑；全程只读，未运行 Cargo、基准、profiling、网络或子代理。