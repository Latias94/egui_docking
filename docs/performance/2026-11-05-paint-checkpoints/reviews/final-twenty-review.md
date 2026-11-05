**数据／源码审查 PASS：最终两组分别满足全部 1,024-pane p50 门槛。** 未发现统计、样本覆盖、工作计数或冻结产物身份错误。完整交付仍需完成中央门禁，并处理下面一项便携证据路径问题。

按最近秩法 `排序值[ceil(n×p/100)−1]`，从原始整数纳秒独立复算了最终两组及两条基线：**四组各 864 行、18 个统计单元，每单元 48 个样本；全部 72 条汇总的 p50／p95／max 均吻合**。样本键完整、无重复，`raw.csv` 与四个进程 CSV 逐行一致，结果文件 SHA-256 全部匹配。

以下为 **egui、1,024 panes** 的结果，单位毫秒，每格为 **p50／p95／max**；click 包含按下、释放两帧。

| 条件 | Idle | Click | Resize |
|---|---:|---:|---:|
| qualified-twenty-one | **8.315083**／8.647834／8.691291 | **30.346125**／32.549667／32.962791 | **18.923292**／20.046625／21.447583 |
| qualified-twenty-two | **8.296125**／9.252208／9.594667 | **30.135626**／34.344542／35.712625 | **18.865250**／19.678041／25.115667 |
| 合并，96 样本 | 8.301667／8.946500／9.594667 | 30.241292／34.177292／35.712625 | 18.914750／20.046625／25.115667 |

两个产品的全部九种规模／场景组合均已核验；合并后的全部 18 行也与 [final-pooled-summary.csv](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-05-paint-checkpoints/final-pooled-summary.csv) 完全一致。资格限于 **12／35／20 ms 的 p50**；resize p95 和最大值仍保留超限样本。

来源与比较条件核对通过：

- **工作量一致。** 最终两组分别对比 `baseline-fork`、`baseline-fork-lto`，八个工作计数字段共 **27,648 次比较，零差异**，包括 click 原有的 draw-command 变化。
- **源码一致。** [candidate-twenty 清单](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/candidate-twenty-source-inventory.json) 完整覆盖当前 **556 项** Rust／manifest／lock 输入，全部哈希匹配；followup 清单与它完全相同。Rust、C++ 工作负载与 `36f3fb0` 逐字节一致，另核对通过三个 benchmark 输入和十个 ImGui 输入哈希。
- **冻结产物一致。** 两组 Rust 均匹配 `47db8ca79dab9b4ae0b53fe19fd69cc40dc94d01efc710ee5bfdd3d7ffc5a12a`；共享 C++ 均匹配 `15b40045bbd19bf24f1d9f029923f43cf5ae14e5d4d34a8645864491f88b1909`。当前 Cargo 日志指出的 production 产物也与冻结 Rust 二进制相同。
- **旧、新优化设置可比。** 旧基线通过 `--release` 加环境变量启用 ThinLTO、一个 codegen unit，与当前 `production` 的有效设置相同；60 个 artifact key 的已记录 profile、features 均一致，registry 包版本无差异。C++ ThinLTO 来源可追溯到 `qualified-one/environment.json`。相对匹配基线，最终两组 p50 降幅约为 idle **73.2%–73.3%**、click **63.0%–63.3%**、resize **55.2%–55.3%**。
- **已记录任务没有时间重叠。** 两组各自为串行 ABBA，测量窗口互不重叠，也没有与已记录的构建、测试或采样区间重叠。最终构建结束于 `15:47:52 UTC`，正式组始于 `15:49:56` 和 `15:59:54`，followup 门禁始于 `16:01:03`。

源码检查与已有 centroid、topology 独立审查一致：此次复用的是私有叶子排列；当前叶子及父节点包围盒、精确查询、邻接顺序、密集回退和 fragments 仍完整计算，调用保持在原计时边界内。旧排列可能降低空间局部性，最坏查询复杂度仍可能为二次。

**剩余问题与资格边界：**

1. **P2：便携证据中的三个来源相对路径失效。** 两份最终环境文件的 `commands`、`rust_build`、`source_inventory` 指向 `../candidate-twenty-*`，实际文件位于 `../provenance/candidate-twenty-*`。见 [第一组环境文件](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-05-paint-checkpoints/qualified-twenty-one/environment.json:11) 和 [第二组环境文件](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-05-paint-checkpoints/qualified-twenty-two/environment.json:11)。文件本身齐全且与运行时原件完全一致；应修正便携副本路径，或提供明确的路径映射以保留原始元数据。
2. **报告已在审查期间更新。** 最新主报告、README、最终统计和合并 CSV 已切换到 `qualified-twenty-one/two`。`qualified-two/three` 已归为中间结果；delivery rebuild 的 resize **20.230167／20.346083 ms** 两次失败仍保留，原始文件也已完整归档。
3. **中央门禁仍在运行。** 截至 **2026-11-06 00:06:02（UTC+08:00）**，[followup 记录](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/qualification-followup/validation-results.json) 为 **5／22 项完成且返回码均为 0**，下一项为 `product-default`。先前 22 项通过属于 `e73d86a`；当前完整资格需待本轮收齐，并同步归档 followup 记录。

本轮仅只读检查并在内存中复算，未修改文件，未运行 Cargo、测试、profiling 或测量。