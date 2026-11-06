**未发现阻塞问题。** 文档中的数值、工作量一致性、源码与二进制配对，以及最终检查记录均有证据支持。结论应保留现有范围：捕获条件下的 CPU 收益，伴随明确的分配与保留存储增量。

已运行独立 Python 校验器，结果通过：验证了 86 个归档文件的哈希、1,728 行前后产品数据、864 行产品参考数据、240 行 headless 进程摘要及 360 行分配记录。此外，逐进程 CSV 与合并结果完全一致，正文各数值表也与数据相符。

- **产品数值和工作量一致。** 重算最近秩 p50/p95/max 后，正文所有产品单元格和降幅均吻合。1,024-pane 的关键结果如下；前后每个样本的 frames、passes、content calls、visible contents、vertices、indices 和 draw commands 相同。[产品摘要](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-linked-targets/frozen-product/summary.csv:8)

  | 场景 | 第一组，毫秒 | 降幅 | 第二组，毫秒 | 降幅 |
  |---|---:|---:|---:|---:|
  | idle | 7.508750 → 7.267500 | 3.21% | 7.567667 → 7.327292 | 3.18% |
  | click | 25.825250 → 25.347084 | 1.85% | 25.992667 → 25.461416 | 2.04% |

  单独产品参考的 **6.48× idle、11.56× click、15.03× resize** 也计算正确；文档将其限定为完整框架／产品比值，未将其归因于纯 docking core。[参考摘要](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-linked-targets/candidate-product/summary.csv:8)

- **来源配对成立。** 565 项清单的键集合完全一致，基线哈希全部匹配 `2935571` Git 对象；候选仅改变指定五个 Rust 文件，当前文件哈希全部匹配候选清单。三项额外 benchmark／runner 来源也匹配基线。headless 的 372 项输入同时匹配 `3803549` 和 `2935571`，支持复用旧可执行文件。实际读取四个冻结二进制后，其 SHA-256 均匹配归档及原始环境记录；候选的提交前构建标签由精确源码清单补足。[来源记录](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-linked-targets/baseline-provenance.json)、[候选构建清单](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-linked-targets/candidate-product/environment.json)

- **headless 和内存表述准确。** 四个基线 idle 进程中位数为 1.859–1.903 ms，候选为 1.633–1.655 ms；正文保留各进程 p50，没有混池。DHAT 数据支持 idle 分配事件维持 2,793、重建更新增加 **13 次分配／270,272 请求字节**，以及一／二／三个保留场景对应 **73,760／147,520／221,280 字节**增量。settlement 的分配事件和请求字节未变，结束时回到单场景存储增量；这些指标的 RSS 限制也已明确。[进程摘要](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-linked-targets/headless/process-summaries.csv)、[分配成本说明](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-linked-targets.md:73)

- **审查、最终门禁和计时隔离有记录支持。** 独立实现审查前后五文件哈希一致。12 项最终检查均成功；压缩日志支持 **2,173 workspace、292／335 host、556 native、10 doctest**，且 59 项 focused 测试确实包含在 workspace 集合中。Clippy 的 correctness／suspicious、格式、依赖边界和 diff 检查记录相符。20 个正式 CPU 测量进程串行执行，两套前后比较均为 ABBA／BAAB；计时区间避开记录中的构建、测试、审查和活跃 DHAT。headless 构建位于产品计时结束之后、headless 计时之前。[最终检查回执](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-linked-targets/checks.json)、[独立审查](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-linked-targets/code-review.md)

**小幅收益和退化必须继续保留。** 第二组 1,024-pane resize 仅改善 **0.30%**；16-pane resize 两组的 p95/max 都上升，第二组 128-pane idle 最大值也上升。headless 最后一轮候选 drag p50 为 **12.847875 ms**，高于基线范围；候选 measurement-change 最大值达 **19.389584 ms**，高于基线最大值 **12.728125 ms**。这些均与正文一致，不能据此声称尾部普遍改善。[退化所在原始摘要行](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-06-linked-targets/headless/process-summaries.csv:236)

证据的主要限制也已如实披露：headless 未保留逐样本时间，无法独立重算其分位数；桌面调度和电源状态未隔离，约 3% 的产品收益应限定于已捕获运行；新增回归对重复／变化 bounds 的直接覆盖，不能扩展为所有边界组合均已穷举测试。本次保持只读，未运行 Cargo、测试或基准，未启动代理。