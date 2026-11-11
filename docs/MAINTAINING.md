# Maintaining Dockspace

## Names and repository structure

The product and GitHub repository are **Dockspace**
([`Latias94/dockspace`](https://github.com/Latias94/dockspace)). The primary
library is UI-framework-neutral; egui is an adapter.

Keep these names distinct:

| Name | Role |
| --- | --- |
| `dockspace` | Core crate and repository slug |
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

Both public crates are currently source-only. Do not advertise a crates.io
installation command or docs.rs version badge until that release exists.

1. Finalize naming and package metadata, include the declared license texts in
   each distributable package, check third-party notices, and write user-facing
   release notes for the selected source revision.
2. Run the CI gates and applicable native qualification. Check both default and
   persistence consumers and keep the documented platform limits accurate.
3. Inspect package contents. `dockspace` must include its command example and
   host guide; `egui_dockspace` must include its application examples and guide.
4. Publish `dockspace` first. Then verify and publish `egui_dockspace` against
   that exact released core version. A local extracted-pair test does not prove
   registry publication.
5. Add installation/version links and release notes after verifying the registry
   packages and generated documentation. Keep the native adapter unpublished
   until it has a separate release decision.

Useful package inspection commands:

```sh
cargo package -p dockspace --all-features --locked --list
cargo package -p egui_dockspace --all-features --locked --list
```

Package verification is a separate step from listing. Before registry release,
the adapter needs the matching core version to resolve successfully. Publish and
verify the core first, then run `cargo package -p egui_dockspace --all-features
--locked` before publishing the adapter. Listing archive contents alone does not
verify either package's build.
