**统计与工具审查通过；两组最终 p50 均达标。完整交付资格仍待中央 22 项门禁完成。** 未发现最终数据的统计、覆盖、工作负载或二进制身份错误。

按最近秩法 `排序值[ceil(n×p/100)−1]` 独立复算了两组最终数据及两条基线：四组各 **864 行、18 个统计单元，每单元 48 个样本**；全部 72 条汇总的 p50、p95、max 和工作负载计数 p50 均吻合。样本键完整且无重复，`raw.csv` 与各进程输出逐行一致，结果文件 SHA-256 全部匹配。

以下为 **1,024 panes 的 egui 结果**，单位毫秒，每格为 **p50 / p95 / max**；click 是按下与释放两帧耗时之和。

| 条件 | Idle | Click | Resize |
|---|---|---|---|
| baseline-fork | 30.965834 / 31.949667 / 35.485625 | 83.319626 / 85.473416 / 85.870042 | 43.529750 / 44.487542 / 53.783917 |
| baseline-fork-lto | 31.072666 / 40.990917 / 48.049750 | 82.009292 / 83.750874 / 90.851209 | 42.223375 / 43.199334 / 50.080958 |
| qualified-two | **8.146417** / 9.698250 / 15.312167 | **30.276709** / 33.458792 / 37.842124 | **19.376500** / 20.073875 / 20.206458 |
| qualified-three | **8.400708** / 21.494958 / 21.971375 | **30.087459** / 32.356042 / 33.043667 | **19.567459** / 21.129500 / 26.856250 |

两组分别满足 **12／35／20 ms** 的 p50 门槛。相对匹配 ThinLTO 基线，idle、click、resize 的 p50 降幅分别约为 **73.0%–73.8%、63.1%–63.3%、53.7%–54.1%**。全部规模和两个产品均参与核验，见 [qualified-two 原始数据](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/qualified-two/raw.csv) 与 [qualified-three 原始数据](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/qualified-three/raw.csv)。

工作负载与构建来源核对通过：

- **逐样本工作量完全一致。** 两组最终数据分别对比 `baseline-fork`、`baseline-fork-lto`，`tabs_per_pane`、`frames`、`passes`、`content_calls`、`visible_contents`、`vertices`、`indices`、`draw_commands` 共 **27,648 次字段比较，零差异**。
- **冻结二进制身份正确。** 两组 Rust 均为 `candidate-seventeen-egui_dockspace`，SHA-256 为 `fde32b76ff88d111dc5ec9f3df8a63e10304acbaae28fe5ac26c74b1df1fa377`；C++ 均为 `qualified-dear_imgui`，SHA-256 为 `15b40045bbd19bf24f1d9f029923f43cf5ae14e5d4d34a8645864491f88b1909`。磁盘文件、元数据及 Rust 构建日志指出的产物一致。
- **来源链区分正确。** [candidate-seventeen 构建记录](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/candidate-seventeen-commands.json) 与构建日志确认 `production` 构建成功、依赖锁定到 fork `60c40304…`。`qualified-one/environment.json` 仅用于追溯 C++ ThinLTO 构建；其旧 Rust resize p50 为 **20.405625 ms**，没有计入最终资格。

永久配置及工具选择逻辑未发现阻塞问题：

- [根工作区](/Users/frankorz/codes/rust/egui_docking/Cargo.toml:28)、[fork 工作区](/Users/frankorz/codes/rust/egui_docking/integration/egui-fork-workspace/Cargo.toml:9) 和 [official 产品工作区](/Users/frankorz/codes/rust/egui_docking/integration/egui-product-harness/Cargo.toml:4) 的 `production` 均继承 release，并设置 ThinLTO、一个 codegen unit。
- [compare_products.py](/Users/frankorz/codes/rust/egui_docking/scripts/compare_products.py:115) 正确区分 official／fork manifest、target 和 profile；Rust 使用 Cargo 返回的实际产物路径。C++ production 分支分别选择 Clang ThinLTO、GCC LTO、MSVC `/GL` 与 `/LTCG`。本轮实际构建证据覆盖 Apple Clang；另外两个分支仅作静态核对。
- 源码清单完整覆盖 **556 项 Rust／manifest／lock 文件，包含全部 5 个未跟踪源码文件**。Rust、C++ 两份产品工作负载源码与 `36f3fb0` **逐字节一致**；脚本及十项 ImGui 输入哈希也符合来源记录。

需要保留以下证据边界：

1. **当前工作树存在一项已记录的测试变化。** 复核期间中央将 `host_authority.rs` 的两个精确工作量断言从 4 更新为 2。因此当前树相对 candidate-seventeen 清单为 **555／556 项一致**；差异仅在 `cfg(test)` 文件，生产源码保持冻结状态。更新后的 [资格源码清单](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/qualification/final-source-inventory.json) 与当前树全部一致，[变更记录](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/qualification/test-only-qualification-update.json) 保留了前后哈希。
2. **中央门禁尚未全部完成。** 首轮断言失败已归档；修正后 `workspace-all` 的 **2,145 项全部通过**。截至 **2026-11-05 22:56:33（UTC+08:00）**，[中央结果](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/qualification/validation-results.json) 记录 **2／22 项门禁通过**，正在执行 `host-serde`。
3. **达标结论限于 p50。** 第三组 idle p95 为 21.494958 ms，两组 resize p95 均超过 20 ms；不能据此宣称尾延迟也满足相同门槛。测量范围仍是 CPU 完整绘制数据生成。
4. 七份既有静态审查均报告无阻塞项，新返回的 [owned-output-review.md](/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/owned-output-review.md) 也已通过；这些静态结论不能替代尚未完成的运行门禁。

工具调用简报：本轮仅使用 `exec_command` 读取本地源码、日志、JSON／CSV，并在内存中复算统计与 SHA-256；未联网、未修改文件，未运行 Cargo、测试、性能分析或测量。