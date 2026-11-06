**发现 1 项 P3，涉及测试 oracle 的错误优先级；未发现生产实现的 P1/P2 正确性问题。**

1. **[P3] minimums 路径的 oracle 包装器没有冻结正确的 override 校验顺序。**  
   位置：[reference.rs:510](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/layout/reference.rs:510)。

   `reference::project` 对两种 `LeafMeasurements` 都先校验 overrides，再测量。但基线与当前的 minimums 路径均先完成测量，再由 `RootMeasurement::project[_with_index]` 校验 overrides。

   具体反例：有效 split 的某个叶子缺少 minimum，同时传入一个 source 有效、weights 为空的 override。原 minimums 路径首先返回 `MissingLeafConstraints`，该 oracle 首先返回 `SplitWeightOverrideLengthMismatch`。当前属性测试在 [topology_tests.rs:78](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/layout/topology_tests.rs:78) 始终传空 overrides，因此没有暴露这个差异。

   **最小修复建议：**让 oracle 的 `Minimums` 分支先调用冻结的 `measure_root`，再调用其已有的 `project`；保留 `Constraints` 分支先校验 override 的顺序。补充上述组合错误用例，并分别覆盖缓存首次编译和预热后的调用。

其余重点核查结果：

- **遍历与数值等价性：**冻结 oracle 的测量状态、测量 DFS、投影 DFS 与基线对应代码逐字一致。新结构编译保留了“进入时检查、子节点按原顺序遍历、退出时组合”的顺序；缓存不匹配后重新完整编译，不会让后方结构错误抢占前方测量错误。投影仍按原 DFS 推进，各输出 map 和数组的取值顺序保持一致。`solve_axis`、约束组合、override 校验及分配检查均未修改。
- **缓存有效性：**[topology.rs:164](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/layout/topology.rs:164) 检查 root、central、完整节点身份、节点种类、轴、子节点数量与顺序、权重数量及原始位模式。固定入口和全部有序边的一致性足以覆盖可达结构；节点代际失效会使节点查询或身份比较失败。未发现同版本、节点重用或 stale hint 被当作布局权威的路径。
- **OnceLock、Clone 与 Eq：**[workspace.rs:67](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/workspace.rs:67) 保留全部原有字段的相等判断，仅忽略 hint。已初始化槽位的克隆共享不可变 topology；并发首次测量可以重复编译，但各调用始终返回自己已校验的结果，`set` 失败不会换用另一线程的结构。未发现影响回滚逻辑或并发测量结果的问题。
- **性能与覆盖边界：**缓存命中后的匹配不构造遍历集合，测量和投影复用临时向量容量；保留的各 split 输出数组有明确用途。新增测试仍缺少节点代际重用、root 节点替换、已初始化索引的 Clone，以及并发首次初始化的直接覆盖；静态检查未发现这些路径存在实现缺陷。

本次仅做只读静态审查，未运行 Cargo、benchmark、采样、联网或派发代理，未修改文件。