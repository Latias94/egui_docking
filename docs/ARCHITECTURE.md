# egui_docking 设计与架构（冻结文档）

本文档用于“冻结”当前 `egui_docking` 的设计目标、架构边界、关键交互规则与后续里程碑，避免未来演进偏离方向。

## 目标（What）

把 `egui_tiles`（dock tree/tiles 模型）与 `egui` multi-viewport（多原生窗口）“打通”，形成编辑器级的完整工作流：

- Tab tear-off 成原生窗口（native viewport）
- 原生窗口跨屏拖动
- 再拖回任意 dock，完成 re-dock
- 支持窗口↔窗口之间移动 tab/subtree（detached ↔ detached）
- 交互尽量贴近 Dear ImGui Docking（DockSpaceOverViewport 心智模型）

## 非目标（What not）

- 不追求 100% 复刻 ImGui 的内部实现细节；追求“体验对齐 + 可维护”的 Rust/egui 风格实现。
- 不强依赖具体渲染后端；仅依赖 egui/eframe 公共 API。
- 不在当前阶段解决所有“输入穿透/遮挡”问题（egui 在同 viewport 内的 z-order/遮挡仍有限制）。

## 仓库与 Crate 组成（Where）

- `egui_docking`（本仓库根 crate）
  - 职责：跨 viewport 的桥接层 + 交互状态机 + overlay 指示/落点决策 + detached viewport 生命周期管理。
  - 关键文件：`src/multi_viewport.rs`
- `egui_tiles_docking`（fork）
  - 职责：为桥接层暴露/补齐 `egui_tiles` 的必要 API（subtree 抽取/插入、dock zone 查询、拖拽 id 覆盖 root tab-bar 等）。
  - 位置：`repo-ref/egui_tiles_docking`（你维护的 fork）
  - 注意：crate 名为 `egui_tiles_docking`，但 `lib` 名保持 `egui_tiles`，实现“导入路径不变”的 drop-in。

## 术语（Terminology）

- **Root viewport**：`ViewportId::ROOT` 对应的主原生窗口。
- **Detached viewport**：通过 `Context::show_viewport_immediate` 创建的子原生窗口；每个窗口里也有一棵 `egui_tiles::Tree`。
- **Tree**：`egui_tiles::Tree<Pane>`，dock 的结构树。
- **SubTree**：一组以某个 `TileId` 为根的 tile 子树（含 descendants）。
- **Overlay targets**：ImGui 风格的落点按钮（Center/Left/Right/Top/Bottom）。
- **Dock zone**：`egui_tiles::Tree::dock_zone_at(...)` 给出的默认“最近落点”区域与 preview rect。

## 当前架构（How）

### 1) 视口与 Tree 管理

- Root tree：`DockingMultiViewport::tree`
- Detached trees：`DockingMultiViewport::detached: BTreeMap<ViewportId, DetachedDock<Pane>>`
  - `DetachedDock { tree, builder }`
  - `builder` 持久保存 title/size/pos 等 viewport 参数

### 2) 跨 viewport 拖拽载荷（payload）

跨窗口拖拽使用 `egui::DragAndDrop` 的 typed payload：

- `DockPayload { bridge_id, source_viewport, tile_id }`
  - `bridge_id`：用来隔离不同 `DockingMultiViewport` 实例（同 app 多套 dock 不串线）
  - `tile_id`：`None` 表示“拖动整个 detached 窗口的 tree”（来自窗口标题栏）；`Some(tile)` 表示“拖动某个 tile/subtree”（来自 tab/pane/或 tab-bar 背景）

### 3) 关键现实问题：mouse-up 事件可能只到“源窗口”

部分后端/平台会出现“拖拽过程中鼠标捕获（capture）”，导致 mouse-up 只在源 viewport 触发。

为保证“松手发生在目标窗口也能完成 drop”，引入：

- `pending_drop: Option<PendingDrop { payload, pointer_global }>`

流程：

1. 任意 viewport 检测到 `pointer.any_released()` 时，如果 payload 存在且指向另一个 viewport，则记录 `PendingDrop`
2. 一帧结束（所有 trees 都跑过 `tree.ui`、rects 都计算完）后在 root `ui()` 末尾统一 `apply_pending_drop(...)`

### 4) Overlay 与默认行为的权威划分（Authority）

核心规则（稳定心智模型）：

- **跨 viewport**：以 `egui_docking` 为权威（`egui_tiles` 无法跨窗口 drop）。
- **同 viewport**：
  - 默认以 `egui_tiles` 内置拖拽/重排/落点为准（最少惊讶原则）。
  - 只有当用户明确 hover 在 overlay target（按钮小方块）上时，overlay 才成为权威落点。

为避免出现“按钮高亮 + 默认 dock preview 高亮”两套提示冲突：

- 当 overlay hovered 时，`egui_docking` 写入一个临时 flag（egui temp data）
- `egui_tiles_docking` 在绘制默认 drag preview 时读取该 flag 并跳过 `paint_drag_preview`

### 5) 内部 overlay 落点的应用方式（不破坏 reorder）

同窗口 overlay 只在“松手落在按钮命中框”时生效：

- 松手时如果命中 overlay 按钮：抽取 subtree（不预留 id-range）→ 插入到目标 insertion
- 否则：完全交给 `egui_tiles` 默认逻辑（tab reorder、最近落点等）

为避免 `TileId` 预留导致内部移动浪费 id：

- 跨 tree（detach/attach）使用 `extract_subtree()`（会 reserve disjoint id-range）
- 同 tree 内部重排使用 `extract_subtree_no_reserve()`

## 交互状态机（冻结）

### 当前已实现（M1）

- **Tear-off（松手创建 detached）**
  - 规则：拖动 tab/pane 松手到 dock_rect 外 → 新建 detached viewport（新 tree）
  - 手势增强：
    - `SHIFT`：优先 tear-off parent `Tabs`（整组 tab 一起拆出）
    - `ALT`：即使松手在 dock 内也强制 tear-off（用于避免“总被吸回去”）
- **Re-dock（跨窗口拖回）**
  - detached → root
  - detached → detached
  - root → detached
- **Overlay targets**
  - 绘制在前景层（Foreground）
  - hover 时显示预览 rect（当前为 50/50 split，尽量与 tiles 默认一致）
  - 采用 ImGui 类似的抗抖动 hit-test（半径阈值 + 象限优先）

### 已实现（M2，部分）

- **Contained floating（CTRL tear-off）**
  - Holding `CTRL` while tearing off will create a contained floating window inside the current viewport (instead of a native viewport).
  - Floating windows can be docked back via drag/drop or the `Dock` button.

### 计划实现（M2/M3）

#### M2：Contained floating（类似 egui_tool_windows）

目的：支持“同一个原生窗口内”的浮动工具窗（被 clip、可拖拽/resize、可置顶），作为编辑器常见形态。

对齐 `egui_tool_windows` 的关键点：

- 约束在容器 `clip_rect` 内
- 自维护 z-order（点击置顶）
- 处理遮挡的输入限制（必要时接受现状限制，或在我们实现里做更强隔离）

#### M3：Ghost viewport（ImGui-like live tear-off）

目的：拖拽过程中就产生“跟随鼠标的浮动窗口”，松手决定“dock 回去”还是“保留浮动/升级原生窗口”。

推荐交互：

- 拖出 dock 边界超过阈值 → 先生成 contained floating ghost
- 继续拖出当前原生窗口边界 → 自动升级为 native viewport
- 松手：
  - 落在任意 dock：回收并 dock
  - 不落在 dock：保留为 floating / detached

## 参考实现与文档（References）

- Dear ImGui Docking（落点计算与渲染）
  - `repo-ref/dear-imgui-rs/dear-imgui-sys/third-party/cimgui/imgui/imgui.cpp`
    - `DockNodeCalcDropRectsAndTestMousePos`：落点 rect + 抗抖动 hit-test
    - `DockNodePreviewDockRender`：前景层 overlay 绘制与颜色策略
- egui multi-viewport API
  - `egui::Context::show_viewport_immediate`
  - `egui::ViewportId`, `egui::ViewportBuilder`, `egui::ViewportCommand`
  - `egui::DragAndDrop`（跨 viewport typed payload）
- egui_tiles（dock tree）
  - `repo-ref/egui_tiles_docking`（fork 后 API：`dock_zone_at`, `extract_subtree`, `insert_subtree_at`, `dragged_id_including_root` 等）
- contained floating window 参考
  - `repo-ref/egui_tool_windows`

## 兼容性与发布策略（Policy）

- `egui_tiles_docking`：作为 fork 发布，保持 `lib` 名为 `egui_tiles`，减少生态迁移成本。
- `egui_docking`：作为桥接 crate 发布；对外 API 尽量小而稳定（主要是 `DockingMultiViewport` + options）。
- 行为变更必须更新本文件的“交互状态机（冻结）”章节，避免 UX 漂移。
