# Third-party sources

This repository uses the following projects as behavioral and protocol references.
Copied algorithms or fixtures retain their original notices at the point of use.

| Project | Use | License |
| --- | --- | --- |
| Open GPUI / Zed GPUI at `67f0048d681e0c44dd7c2e70c8d6386f42b3601b` | Dock graph, checked transaction, drop-target, and viewport protocol semantics | Apache-2.0, Copyright 2022-2025 Zed Industries, Inc. |
| Dear ImGui docking at `81c008f90` | Dock request ordering, central-node, preview/delivery, and platform viewport protocol semantics | MIT, Copyright 2014-2026 Omar Cornut |
| Dockview at `0006ab0a18a1e9168e4ae6066a7402250da1d6fc` | Split normalization and failed-popout regression behavior | MIT, Copyright 2021 mathuo; relevant grid/split sources also attribute Microsoft VS Code under MIT |
| egui and eframe | Rendering and native viewport integration | MIT OR Apache-2.0 |

The legacy implementation in this repository is characterization evidence only and is
not a compatibility target for the rewritten public API or persistence schema.
