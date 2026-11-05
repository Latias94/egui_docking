**复算未发现分位数、样本覆盖或工作负载计数错误；发现一个归档缺口：candidate-six 的冻结可执行文件目前无法复核。** 本轮只读取已有文件并离线计算，没有修改文件，也没有运行 Cargo、测试、采样或性能测量。

九组主数据各有 **864 条数据行，不含表头**，合计 **7,776 条**。每组均为四个进程、每进程 216 行；两个产品 × 三种规模 × 三种场景，共 18 个分组，每组汇集 48 个样本。样本键完整、无重复，`raw.csv` 与四份进程输出逐行一致。

独立采用最近秩法 `排序值[ceil(n × p / 100) − 1]` 重算后，九组全部 **162 条 summary 记录**的 p50、p95、最大值及工作负载计数 p50 均吻合；原始文件和汇总文件的 SHA-256 也全部符合元数据记录。

下表为 1,024 panes 的 egui 结果，单位为毫秒，每格为 **p50 / p95**。Click 是按下、释放两帧耗时之和。

| 条件 | 原始行数 | Idle | Click | Resize |
| --- | ---: | ---: | ---: | ---: |
| baseline-fork | 864 | 30.965834 / 31.949667 | 83.319626 / 85.473416 | 43.529750 / 44.487542 |
| candidate-one | 864 | 11.921000 / 12.353292 | 45.437750 / 46.755375 | 24.315042 / 27.686791 |
| candidate-two | 864 | 10.149708 / 10.810250 | 43.305709 / 44.089999 | 23.090500 / 23.645708 |
| candidate-three | 864 | 9.689041 / 10.925292 | 41.532875 / 42.883000 | 22.119792 / 22.697084 |
| candidate-four | 864 | 9.552667 / 10.239000 | 37.904709 / 40.239125 | 22.456542 / 23.569958 |
| candidate-five | 864 | 9.549167 / 11.143916 | 36.851209 / 38.592374 | 21.361417 / 22.233333 |
| candidate-six | 864 | 9.570791 / 10.320417 | 34.218251 / 34.898417 | 21.888750 / 22.419375 |
| candidate-seven | 864 | 9.226083 / 9.851250 | 32.378791 / 34.409125 | 20.394792 / 22.145250 |
| baseline-fork-lto | 864 | 31.072666 / 40.990917 | 82.009292 / 83.750874 | 42.223375 / 43.199334 |

Dear ImGui、16 panes 和 128 panes 的全部分组也已参与上述核验。

九组的工作负载字段与 baseline-fork **逐行完全一致**，不只是中位数一致。每组 864 行的累计计数均为：

| 字段 | 每组累计值 |
| --- | ---: |
| frames / passes | 1,152 / 1,152 |
| content_calls / visible_contents | 448,656 / 448,512 |
| vertices | 74,918,784 |
| indices | 152,767,584 |
| draw_commands | 1,348,056 |

所有行都满足两标签、场景帧数、完整可见内容及正数绘制输出要求。egui Click 的绘制命令数存在正常的逐样本变化，例如 1,024 panes 为 10,243 或 10,244；该变化在九组间也逐行相同。

dense 确认位于 [dense-key-confirmation/raw.csv](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/dense-key-confirmation/raw.csv)。实际为 **576 行**，顺序是 four/five/five/four，每次 144 行；每个条件 288 行、每个条件和场景 96 个样本，仅包含 1,024 panes 的 egui。

| dense 条件 | Idle p50 / p95 | Click p50 / p95 | Resize p50 / p95 |
| --- | ---: | ---: | ---: |
| candidate-four | 9.301292 / 10.227042 | 37.099000 / 37.951208 | 21.996292 / 23.571000 |
| candidate-five | 9.224875 / 10.244875 | 36.783834 / 38.156667 | 21.346333 / 22.129708 |

逐次 Resize p50 为 four **21.797292**、five **21.258583**、five **21.420209**、four **22.393500**，因此[报告中的“两次 five 均低于两次 four”结论成立](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-05-paint-checkpoints-results.md:82)。四份进程输出能完整重建 raw；六条汇总全部重算吻合，且与 [summary-recovery.json](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/dense-key-confirmation/summary-recovery.json) 的两个文件摘要一致。

对[结果报告](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-05-paint-checkpoints-results.md)，需要保留以下来源与结论边界：

- **candidate-six 二进制归档不完整。** 审阅目录没有 `candidate-six-egui_dockspace`；其[元数据](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/candidate-six/environment.json:691)指向共享 target，当前该文件哈希已不符合记录。因此[“所有冻结可执行文件均保存在此目录”的表述](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-05-paint-checkpoints-results.md:102)需要收窄。其余已保存的对应可执行文件哈希均吻合。
- **构建条件必须分开。** candidate-seven 和 baseline-fork-lto 记录了 Rust ThinLTO、一个 codegen unit，以及 C++ ThinLTO。candidate-seven 的 556 项源码清单与 candidate-six 的对应清单完全相同；应与匹配的 LTO 基线比较，不能把 six→seven 的差值直接计为源码优化收益。报告现有普通 release 表格正确对应 baseline 至 candidate-six。
- **历史来源仍有明确限制。** baseline 构建命令缺少独立 UTC 起止时间，无法仅从该记录重建构建区间。部分条件的 `source_commit` 仍是基础提交 `36f3fb0`，具体候选状态依赖独立源码哈希清单。
- **570 项 fork 哈希声明已独立核实。** candidate-one 清单与本地 Git 中 `60c40304` 的 458 个普通 blob 内容摘要、112 个 LFS 内容 OID 全部匹配；路径依赖与 Git 依赖的包来源标识仍不同。本轮没有核验远程发布状态。
- **数值一致不等于完整语义资格通过。** 计数支持没有发生 CSV 可见的工作量缩减，不能单独证明所有几何和输入行为等价。报告的 50、125、77、64 项测试通过数均有既有日志支持，本轮未重跑。现有结果仍是阶段性证据，后续最终比较及完整资格检查应另行形成结论。