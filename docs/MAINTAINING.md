# Maintaining Dockspace

## Names and repository structure

The product and GitHub repository are **Dockspace**
([`Latias94/dockspace`](https://github.com/Latias94/dockspace)). The primary
library is UI-framework-neutral; egui is an adapter.

Keep these names distinct:

| Name | Role |
| --- | --- |
| `dockspace` | Core crate and repository slug |
| `dockspace_winit` | Optional window input and repaint helpers for winit hosts |
| `egui_dockspace` | Official egui adapter crate |
| `egui_dockspace_native` | Experimental unpublished native adapter |
| `dockspace_host_conformance` | Shared test-scenario crate |

Keep one repository. Core, adapters and conformance scenarios currently evolve
together, and independent consumer workspaces already test their separation.
Splitting repositories would add release coordination before it has a concrete
benefit. Keep the official and fork-backed Cargo workspaces separate.

Use a concrete GitHub About description, such as "Renderer-neutral docking for
Rust editor UIs, with egui adapters." Suggested topics are `rust`, `docking`,
`egui`, `gui` and `editor`. Point the homepage at a maintained user guide or demo
once one is hosted; avoid linking an internal plan as the product homepage.

## Source modules

Keep state and the operations which advance it together. A file's length alone
does not justify another layer: inspect production code separately from tests.
Use Rust's ordinary `module.rs` and `module/` layout so source navigation follows
module ownership without `#[path]` aliases.

The public entry points are `geometry`, `model`, `policy`, `close`, `runtime`,
and the optional `motion` module. Keep raw protocols inside their owning module;
do not preserve obsolete root paths with compatibility re-exports. Private
protocol tests use the same internal paths as production code and do not require
test-only public module declarations.

| Area | Owner | Responsibilities |
| --- | --- | --- |
| Durable topology | `dockspace/src/graph/` | Graph storage, commands, checked transactions, canonicalization, validation and read-only queries |
| Ordered input | `dockspace/src/input/` | Provider ingress, pointer journals, receiver receipts and frozen output evidence for one journal reduction |
| Gestures and targeting | `dockspace/src/interaction/` | Gesture state, deadlines, target acquisition and immutable drop geometry/resolution |
| Presentation | `dockspace/src/presentation/` | Layout projection, measurement manifests, compiled scenes, tab strips, exact hit geometry and output observation |
| Platform lifecycle | `dockspace/src/platform/` | Provider authority, effects, observed coordinates, window bindings, focus and viewport coordination |
| Atomic publication | `dockspace/src/engine/` | Reducer transactions, pending mutation progress, logical surface recovery, commit events and resource retention |
| Persistence | `dockspace/src/persistence/` | Workspace snapshots, item-key mapping, documents and window placement preferences |
| Native pointer normalization | `egui_dockspace_native/src/pointer_event/` | Device enrollment, mouse button tails and scroll phases before core ingress |
| Framework hit translation | `egui_dockspace_native/src/receiver.rs` | Match exact presented egui artifacts and independently resolve delivery/hover lanes |

Directory grouping does not create another state owner. `DockEngine` remains
the single writer; its presentation authority, pending mutations and platform
coordinator keep their existing publication and cancellation responsibilities.
Shared geometry and stable identities remain small root modules. Public policy
and close modules explicitly export application-facing types while their rule
and plan implementations remain private to the crate.

Keep private tests in the matching `module/tests.rs` or narrowly named child
modules. Test helpers stay private to their module unless multiple production
consumers justify a shared interface. Moving a test must preserve its behavior
and must not widen production visibility just to keep it compiling.

## Documentation ownership

- The root README answers what the project does, how to try it and which host
  to choose. Keep it short and avoid dated implementation history.
- Crate READMEs describe installation, first use and customization. Packaged
  guides retain the complete host contracts.
- `docs/EXAMPLES.md` owns the run-command catalog and explains example limits.
- `CONTRIBUTING.md` owns development commands; CI owns the executable gates.
- `docs/adr/` records the few architectural decisions whose trade-offs are hard
  to recover from code. Keep current contracts in guides, not in old plans.
- Keep build outputs, benchmark samples and dated qualification logs outside the
  source tree. Attach relevant source/environment details to CI artifacts or
  releases. Results do not silently expand platform or performance claims.

## First registry release

The public crates are currently source-only. Do not advertise a crates.io
installation command or docs.rs version badge until that release exists.

1. Finalize naming and package metadata, include the declared license texts in
   each distributable package, check third-party notices, and write user-facing
   release notes for the selected source revision.
2. Run the CI gates and applicable native qualification. Check both default and
   persistence consumers and keep the documented platform limits accurate.
3. Inspect package contents. `dockspace` must include its command example and
   host guide; `dockspace_winit` must include its host setup README;
   `egui_dockspace` must include its application examples and guide.
4. Publish `dockspace` first, then verify and publish `dockspace_winit`, then
   `egui_dockspace` against those exact released versions. A local package test
   does not prove registry publication.
5. Add installation/version links and release notes after verifying the registry
   packages and generated documentation. Keep the native adapter unpublished
   until it has a separate release decision.

Useful package inspection commands:

```sh
cargo package -p dockspace --all-features --locked --list
cargo package -p dockspace_winit --all-features --locked --list
cargo package -p egui_dockspace --all-features --locked --list
```

Package verification is a separate step from listing. Before registry release,
each adapter needs its matching dependencies to resolve successfully. Publish and
verify in dependency order, running `cargo package -p <crate> --all-features
--locked` before publishing each adapter. Listing archive contents alone does not
verify a package's build.
